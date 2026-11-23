--!strict
-- A board is tiles on a four-column grid. Each tile is an ability's `tile` face; this pack
-- brings one ability of its own and borrows the rest from EchoSpot.
local echo = require("@echo")
local t = require("@echo/types")

return echo.pack {
	abilities = {
		week = echo.ability {
			title = "Week of the Year",
			icon = "icons/board.svg",
			kind = "read",
			output = t.number(),
			run = function()
				return tonumber(echo.date("%V"))
			end,
		},
	},
	boards = {
		today = echo.board {
			title = "Today",
			icon = "icons/board.svg",
			tiles = {
				{ "__ID__.week", size = "1x1" },
				{ "calc.eval", args = { expression = "days until 31 dec" }, size = "2x1", title = "Days Left This Year" },
				{ "calendar.next", size = "1x1" },
			},
		},
	},
}
