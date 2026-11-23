--!strict
-- Timeline: every effect a person started, newest first, with Undo where there is a way back.
-- A system pack: it is written on the same API as any other, and `echospot fork activity`
-- copies it to your packs, where the copy takes over.
local echo = require("@echo")
local t = require("@echo/types")
local ui = require("@echo/ui")

local effect = t.record("effect", {
	id = t.number(),
	title = t.text(),
	summary = t.text():optional(),
	address = t.text(),
	t = t.number(),
	undone = t.bool(),
	final = t.bool(),
	can_undo = t.bool(),
})

local function when(e)
	if e.undone then
		return "Undone"
	elseif e.final then
		return "Can't be undone"
	end
	return echo.date("%H:%M", e.t)
end

return echo.pack {
	abilities = {
		recent = echo.ability {
			title = "Timeline",
			description = "What you did lately, and taking it back",
			icon = "icons/timeline.svg",
			kind = "read",
			keywords = { "undo", "history", "effects", "recent" },
			output = t.list(effect),
			run = function(_, cx)
				return cx:call("timeline.list", { limit = 100 })
			end,
			faces = {
				panel = function(_, cx)
					return ui.List {
						items = cx:load("activity.recent", {}),
						title = "title",
						subtitle = "summary",
						accessory = when,
						intent = function(e)
							return if e.undone then "muted" else nil
						end,
						empty = { title = "Nothing Yet", hint = "What you do that changes something shows up here" },
						actions = function(e)
							if not e.can_undo then
								return {}
							end
							return {
								{
									title = "Undo",
									icon = "undo",
									run = function(item, cx)
										cx:call("timeline.undo", { id = item.id })
										cx:toast("Undone", "success")
									end,
								},
							}
						end,
					}
				end,
			},
		},
		undo_last = echo.ability {
			title = "Undo Last Action",
			description = "Take back the newest thing that can be",
			icon = "icons/timeline.svg",
			kind = "effect",
			keywords = { "undo", "revert" },
			output = t.text(),
			run = function(_, cx)
				for _, e in cx:call("timeline.list", { limit = 100 }) do
					if e.can_undo then
						cx:call("timeline.undo", { id = e.id })
						return e.title
					end
				end
				error("Nothing to undo", 0)
			end,
			undo = function() end,
		},
	},
}
