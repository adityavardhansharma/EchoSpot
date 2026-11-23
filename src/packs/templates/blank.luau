--!strict
local echo = require("@echo")
local t = require("@echo/types")

return echo.pack {
	abilities = {
		hello = echo.ability {
			title = "Say Hello",
			icon = "icons/hello.svg",
			kind = "read",
			input = { name = t.text():default("world") },
			output = t.text(),
			examples = { "hello ada" },
			run = function(input, cx)
				return "Hello, " .. input.name .. "."
			end,
		},
	},
}
