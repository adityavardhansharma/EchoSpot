--!strict
-- Timeline, ejected: things in time order, on a rail.
-- `local Timeline = require("@self/ui/Timeline")` then `Timeline { items = events, title = "title", time = "at" }`.
local ui = require("@echo/ui")

-- A piece of the rail; the first and last dots have nothing beyond them.
local function segment(shown: boolean, height: number): any
	if shown then
		return ui.box({ width = 2, height = height, fill = "separator", padding = "none", radius = "thumb" }, {})
	end
	return ui.col({ width = 2, height = height, grow = false }, {})
end

return function(p: { [string]: any })
	local items, st, err = ui.resolve(p.items)
	if st == "pending" then
		return ui.col({ gap = "md" }, ui.each({ 1, 2, 3 }, function()
			return ui.box({ height = 40, radius = "row", fill = "card", intent = "loading" }, {})
		end))
	elseif st == "error" then
		return ui.Empty { title = "Couldn't Load", hint = tostring(err), state = "error" }
	elseif #(items or {}) == 0 then
		local empty = if type(p.empty) == "string" then { title = p.empty } else p.empty
		return ui.Empty(empty or { title = "Nothing Yet", icon = "clock" })
	end
	return ui.col({ gap = "none" }, ui.each(items, function(item, i)
		local intent = ui.pick(item, p.intent, i)
		local actions = if type(p.actions) == "function" then p.actions(item, i) else p.actions
		local first = actions and actions[1]
		local run = p.press or (first and first.run)
		return ui.row({ gap = "lg", px = "md", grow = true, radius = "row", intent = intent, press = if run then function(_, cx) run(item, cx) end else nil }, {
			-- The rail: a dot on a line that runs the length of the list.
			ui.col({ gap = "xxs", align = "center", grow = false }, {
				segment(i > 1, 10),
				ui.box({ width = 8, height = 8, radius = "thumb", fill = ui.pick(item, p.tint, i) or "accent", padding = "none" }, {}),
				segment(i < #items, 22),
			}),
			ui.col({ gap = "xxs", grow = 1, py = "sm" }, {
				ui.text(ui.pick(item, p.title, i) or (if type(item) == "table" then item.title or item.name else tostring(item)) or "", { role = "body" }),
				ui.text(ui.pick(item, p.subtitle, i) or "", { role = "caption", color = "text-secondary" }),
			}),
			ui.text(ui.pick(item, p.time, i) or "", { role = "caption", color = "text-secondary" }),
		})
	end))
end
