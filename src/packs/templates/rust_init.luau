--!strict
-- The pack's UI. The abilities are Rust (src/lib.rs); a Rust pack is headless, so what it shows
-- beyond the default faces is drawn here, loading the Rust abilities through the broker.
local echo = require("@echo")
local ui = require("@echo/ui")

return echo.pack {
	surfaces = {
		explore = echo.panel {
			title = "Primes",
			icon = "icons/prime.svg",
			render = function(view, cx)
				local primes = cx:load("__ID__.primes", { limit = 200 })
				return ui.Grid {
					items = primes,
					title = function(p) return tostring(p) end,
					icon = function() return "hash" end,
					columns = 8,
					empty = { title = "No Primes", hint = "Nothing below the limit" },
				}
			end,
		},
	},
}
