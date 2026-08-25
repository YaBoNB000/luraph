// Minimal luau-compile: syntax/bytecode validation for the luraph matrix.
// Writes the bytecode blob to stdout; nonzero exit + message on error.
#include <lua.h>
#include <lualib.h>
#include <Luau/Compiler.h>
#include <cstdio>
#include <fstream>
#include <string>

int main(int argc, char** argv)
{
    if (argc < 2)
    {
        fprintf(stderr, "usage: luau-compile <file>\n");
        return 2;
    }
    std::ifstream in(argv[1], std::ios::binary);
    if (!in)
    {
        fprintf(stderr, "luau-compile: cannot open %s\n", argv[1]);
        return 2;
    }
    std::string source((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    std::string bytecode = Luau::compile(source);
    lua_State* L = luaL_newstate();
    if (luau_load(L, argv[1], bytecode.data(), bytecode.size(), 0) != 0)
    {
        const char* m = lua_tostring(L, -1);
        fprintf(stderr, "luau-compile: %s\n", m ? m : "(compile failed)");
        lua_close(L);
        return 1;
    }
    lua_close(L);
    std::fwrite(bytecode.data(), 1, bytecode.size(), stdout);
    return 0;
}
