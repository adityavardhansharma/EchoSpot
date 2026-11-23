--!strict
-- The whole pack. It calls only calc.convert, a free built-in, so it runs at once with no
-- grant screen. The output's type gives it a row in root search, a panel and a board tile.
local echo = require("@echo")
local t = require("@echo/types")

return echo.pack {
	abilities = {
		convert = echo.ability {
			title = "Convert Currency",
			icon = "icons/convert.svg",
			kind = "read",
			input = { amount = t.number(), from = t.currency(), to = t.currency() },
			output = t.money(),
			examples = { "50 usd to inr", "€20 in yen" },
			run = function(input, cx)
				return cx:call("calc.convert", input)
			end,
		},
	},
}
