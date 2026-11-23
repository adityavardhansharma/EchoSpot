--!strict
-- A focus timer in a corner of the screen. The countdown lives in a store, so the overlay is
-- drawn again each time it changes; a trigger moves it along.
local echo = require("@echo")
local t = require("@echo/types")
local ui = require("@echo/ui")

return echo.pack {
	stores = {
		remaining = echo.store { type = t.number(), initial = 0 },
	},
	abilities = {
		start = echo.ability {
			title = "Start Focus Timer",
			icon = "icons/timer.svg",
			kind = "effect",
			input = { minutes = t.number():default(25) },
			output = t.number(),
			examples = { "focus 25", "focus for 50" },
			run = function(input, cx)
				cx:put("remaining", input.minutes * 60)
				return input.minutes
			end,
			undo = function(_, _, cx)
				cx:put("remaining", 0)
			end,
		},
	},
	triggers = {
		tick = echo.trigger {
			every = "1s",
			run = function(_, cx)
				local left = cx:store("remaining")
				if left > 0 then
					cx:put("remaining", left - 1)
					if left == 1 then
						cx:call("system.notify", { title = "Focus time is up" })
					end
				end
			end,
		},
	},
	surfaces = {
		timer = echo.overlay {
			place = "top-right",
			input = "none",
			size = "200x76",
			render = function(view, cx)
				local left = cx:store("remaining")
				if left <= 0 then
					return nil
				end
				return ui.Stat {
					value = string.format("%d:%02d", left // 60, left % 60),
					caption = "Focus",
					intent = if left < 60 then "urgent" else "live",
				}
			end,
		},
	},
}
