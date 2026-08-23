require("./polyfill")
local ok, err = pcall(function()
	local r = require("./luraph15_trace")
	print("trace module returned:", r)
end)
if not ok then
	print("=== OUTER ERROR ===")
	print(err)
end
