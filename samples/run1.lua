-- /home/user/luraph/samples/run1.lua
local pf = require("./polyfill")

local ok, err = pcall(function()
	local result = require("./luraph15")
	print("=== PROTECTED PROGRAM RETURNED ===")
	print("result type:", type(result))
end)

if not ok then
	print("=== RUNTIME ERROR ===")
	print(err)
end

print("=== BUFFER DUMP (decoded data) ===")
pf.dump_all()
