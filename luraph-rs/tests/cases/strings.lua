-- strings.lua: escapes, long strings, string.*
local e1 = "tab\tquote\"back\\nl\nend"
print("escapes:", #e1, e1:sub(1, 3))
-- NOTE: \x escapes are Luau-only (5.1 treats \x as literal 'x') —
-- covered in luau_escapes.lua; use decimal escapes here (common to both)
local e2 = "\101\102\103\68\69\0zero"
print("byte esc:", #e2, e2:sub(1, 5), e2:sub(6, 6) == "\0")
-- 5.1 unknown-escape behavior (literal char) — same in this shared corpus
-- only if the target is 5.1; to keep both dialects identical we avoid it.
local ls1 = [[long
string]]
print("long:", #ls1, ls1:sub(1, 4))
local ls2 = [===[eq [[ inside]===]
print("long eq:", ls2:sub(5, 6), #ls2)
-- format
print(string.format("%d-%s-%.2f", 7, "x", 3.14159))
print(string.format("[%s]", "100%"))
print(string.format("%q", "a\"b"))
-- sub / rep / byte / char
local s = "hello world"
print("sub:", s:sub(7, 11), s:sub(1, 5), s:sub(-5))
print("rep:", string.rep("ab", 3), #string.rep("x", 5))
print("byte:", s:byte(1), s:byte(7), string.byte("A"))
print("char:", string.char(72, 105))
-- find / match
print("find:", s:find("world"), s:find("l", 4), s:find("nope") == nil)
print("match:", s:match("(%w+) (%w+)"), s:match(".*l"))
local num = "abc123def456"
print("match num:", num:match("(%d+)"), select(2, num:find("(%d+)%w+")))
-- gmatch
local words = 0
for w in ("one two three"):gmatch("%w+") do
	words = words + #w
end
print("gmatch:", words)
-- gsub
print("gsub:", ("a-b-c"):gsub("%-", "+"), ("abc"):gsub("b", "XY"))
local n, rep = ("hello"):gsub("l", "L")
print("gsub count:", n, rep)
-- concat + #
local big = "x" .. "y" .. "z"
print("concat:", big, #"x" .. "y")
-- unicode (UTF-8) passthrough
local zh = "中文测试"
print("utf8:", #zh, zh:sub(1, 2))
print("strings done")
