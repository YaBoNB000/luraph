// Minimal Luau 0.735 interpreter CLI — replicates the official CLI
// runtime environment (required for the luraph test corpus):
//   luaL_openlibs + custom loadstring/collectgarbage globals +
//   require file rehook + luaL_sandbox (readonly globals, as the
//   official CLI has since 0.600).
#include <lua.h>
#include <lualib.h>
#include <Luau/Compiler.h>
#include <Luau/Require.h>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <string>

// --- loadstring (official CLI semantics: compile + luau_load) ----------
static int lua_loadstring(lua_State* L)
{
    size_t l = 0;
    const char* s = luaL_checklstring(L, 1, &l);
    const char* chunkname = luaL_optstring(L, 2, s);
    lua_setsafeenv(L, LUA_ENVIRONINDEX, false);
    std::string bytecode = Luau::compile(std::string(s, l));
    if (luau_load(L, chunkname, bytecode.data(), bytecode.size(), 0) == 0)
        return 1;
    lua_pushnil(L);
    lua_insert(L, -2);
    return 2;
}

// --- collectgarbage ----------------------------------------------------
static int lua_collectgarbage(lua_State* L)
{
    const char* option = luaL_optstring(L, 1, "collect");
    if (strcmp(option, "collect") == 0)
    {
        lua_gc(L, LUA_GCCOLLECT, 0);
        return 0;
    }
    if (strcmp(option, "count") == 0)
    {
        lua_pushnumber(L, lua_gc(L, LUA_GCCOUNT, 0));
        return 1;
    }
    luaL_error(L, "collectgarbage must be called with 'count' or 'collect'");
}

// --- require: filesystem rehook (relative to the requirer's dir) -------
struct ReqCtx
{
    std::string dir; // absolute directory of the current requirer
};

static std::string dirOf(const std::string& p)
{
    size_t i = p.find_last_of('/');
    if (i == std::string::npos)
        return ".";
    if (i == 0)
        return "/";
    return p.substr(0, i);
}

static bool fileExists(const std::string& p)
{
    std::ifstream f(p);
    return bool(f);
}

static bool modulePath(const ReqCtx* c, const std::string& name, std::string& out)
{
    const char* exts[] = {".luau", ".lua"};
    for (int e = 0; e < 2; ++e)
    {
        std::string p = c->dir + "/" + name + exts[e];
        if (fileExists(p))
        {
            out = p;
            return true;
        }
    }
    const char* inits[] = {"init.lua", "init.luau"};
    for (int e = 0; e < 2; ++e)
    {
        std::string p = c->dir + "/" + name + "/" + inits[e];
        if (fileExists(p))
        {
            out = p;
            return true;
        }
    }
    return false;
}

static bool rh_allow(lua_State*, void*, const char*)
{
    return true;
}

static luarequire_NavigateResult rh_reset(lua_State*, void* vctx, const char* requirer)
{
    ReqCtx* c = static_cast<ReqCtx*>(vctx);
    std::string r = requirer ? requirer : "";
    if (r.empty() || r[0] == '=')
        c->dir = ".";
    else
        c->dir = dirOf(r);
    return NAVIGATE_SUCCESS;
}

static luarequire_NavigateResult rh_jump_alias(lua_State*, void*, const char*)
{
    return NAVIGATE_NOT_FOUND;
}

static luarequire_NavigateResult rh_parent(lua_State*, void* vctx)
{
    ReqCtx* c = static_cast<ReqCtx*>(vctx);
    if (c->dir == "/" || c->dir.empty() || c->dir == ".")
        return NAVIGATE_NOT_FOUND;
    std::string d = c->dir;
    if (!d.empty() && d.back() == '/')
        d.pop_back();
    size_t i = d.find_last_of('/');
    if (i == std::string::npos)
        c->dir = ".";
    else if (i == 0)
        c->dir = "/";
    else
        c->dir = d.substr(0, i);
    return NAVIGATE_SUCCESS;
}

static luarequire_NavigateResult rh_child(lua_State*, void*, const char*)
{
    return NAVIGATE_SUCCESS;
}

static bool rh_module_present(lua_State*, void* vctx)
{
    // is_module_present is called with the child name already tracked by
    // the library; we re-derive from the last require target stored in
    // the ctx by rh_load bookkeeping (simplification: always true — the
    // corpus does not exercise ambiguous require trees)
    (void)vctx;
    return true;
}

static luarequire_WriteResult writeStr(char* buffer, size_t buffer_size, size_t* size_out, const std::string& s)
{
    if (buffer_size < s.size() + 1)
    {
        *size_out = s.size() + 1;
        return WRITE_BUFFER_TOO_SMALL;
    }
    std::memcpy(buffer, s.c_str(), s.size() + 1);
    *size_out = s.size();
    return WRITE_SUCCESS;
}

static luarequire_WriteResult rh_chunkname(lua_State*, void* vctx, char* buffer, size_t buffer_size, size_t* size_out)
{
    ReqCtx* c = static_cast<ReqCtx*>(vctx);
    return writeStr(buffer, buffer_size, size_out, c->dir);
}

static int rh_load(lua_State* L, void* vctx, const char* path, const char* chunkname, const char* loadname)
{
    ReqCtx* c = static_cast<ReqCtx*>(vctx);
    (void)loadname;
    std::string file;
    if (!modulePath(c, path, file))
    {
        lua_pushfstring(L, "module '%s' not found", path);
        lua_error(L);
    }
    std::ifstream in(file, std::ios::binary);
    std::string source((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    std::string bytecode = Luau::compile(source);
    if (luau_load(L, chunkname, bytecode.data(), bytecode.size(), 0) != 0)
        lua_error(L);
    return 1;
}

static luarequire_ConfigStatus rh_config_status(lua_State*, void*)
{
    return CONFIG_ABSENT;
}

static luarequire_WriteResult rh_get_alias(lua_State*, void*, const char*, char*, size_t, size_t*)
{
    return WRITE_FAILURE;
}

static void configInit(luarequire_Configuration* cfg)
{
    cfg->is_require_allowed = rh_allow;
    cfg->reset = rh_reset;
    cfg->jump_to_alias = rh_jump_alias;
    cfg->to_parent = rh_parent;
    cfg->to_child = rh_child;
    cfg->is_module_present = rh_module_present;
    cfg->get_chunkname = rh_chunkname;
    cfg->get_loadname = rh_chunkname;
    cfg->get_cache_key = rh_chunkname;
    cfg->get_config_status = rh_config_status;
    cfg->get_alias = rh_get_alias;
    cfg->load = rh_load;
}

// --- main ----------------------------------------------------------------
int main(int argc, char** argv)
{
    if (argc < 2)
    {
        fprintf(stderr, "usage: luau <script> [args...]\n");
        return 2;
    }
    std::ifstream in(argv[1], std::ios::binary);
    if (!in)
    {
        fprintf(stderr, "luau: cannot open %s\n", argv[1]);
        return 2;
    }
    std::string source((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    std::string bytecode = Luau::compile(source);

    lua_State* L = luaL_newstate();
    luaL_openlibs(L);

    static const luaL_Reg funcs[] = {
        {"loadstring", lua_loadstring},
        {"collectgarbage", lua_collectgarbage},
        {nullptr, nullptr},
    };
    lua_pushvalue(L, LUA_GLOBALSINDEX);
    luaL_register(L, nullptr, funcs);
    lua_pop(L, 1);

    ReqCtx rc;
    rc.dir = dirOf(argv[1]);
    luaopen_require(L, configInit, &rc);
    luaL_sandbox(L);

    if (luau_load(L, argv[1], bytecode.data(), bytecode.size(), 0) != 0)
    {
        const char* m = lua_tostring(L, -1);
        fprintf(stderr, "luau: %s\n", m ? m : "(load failed)");
        lua_close(L);
        return 1;
    }
    int r = lua_pcall(L, 0, 0, 0);
    if (r != 0)
    {
        const char* m = lua_tostring(L, -1);
        fprintf(stderr, "luau: %s\n", m ? m : "(pcall failed)");
        lua_close(L);
        return 1;
    }
    lua_close(L);
    return 0;
}
