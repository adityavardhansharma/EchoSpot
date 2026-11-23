--!strict
-- List, ejected: the rows drawn with primitives, so every part of one can be changed.
-- Use it in place of ui.List: `local List = require("@self/ui/List")` then `List { items = … }`.
--
-- What is yours now: an ejected list is drawn by this file, so the host no longer gives it
-- keyboard selection or the action bar. Rows answer a press; `actions` is the first action's run.
local ui = require("@echo/ui")

return function(p: { [string]: any })
	local items, state, err = ui.resolve(p.items)
	if state == "pending" then
		return ui.col({ gap = "sm", padding = "md" }, ui.each({ 1, 2, 3, 4 }, function()
			return ui.box({ height = 34, radius = "row", fill = "card", intent = "loading" }, {})
		end))
	elseif state == "error" then
		return ui.Empty { title = "Couldn't Load", hint = tostring(err), state = "error" }
	elseif #(items or {}) == 0 then
		local empty = p.empty
		if type(empty) == "string" then
			empty = { title = empty }
		end
		return ui.Empty(empty or { title = "No Results" })
	end
	return ui.col({ gap = "xxs", padding = "md" }, ui.each(items, function(item, i)
		local intent = ui.pick(item, p.intent, i)
		local actions = if type(p.actions) == "function" then p.actions(item, i) else p.actions
		local first = actions and actions[1]
		return ui.row({
			gap = "lg", px = "md", py = "sm", radius = "row", align = "center", intent = intent,
			press = if first then function(_, cx) first.run(item, cx) end else nil,
		}, {
			ui.icon(ui.pick(item, p.icon, i) or "circle", { tint = ui.pick(item, p.tint, i) }),
			ui.text(ui.pick(item, p.title, i) or item.title or tostring(item), { role = "body" }),
			ui.text(ui.pick(item, p.subtitle, i) or "", { role = "caption", color = "text-secondary" }),
			ui.spacer(),
			ui.text(ui.pick(item, p.accessory, i) or "", { role = "caption", color = "text-secondary" }),
		})
	end))
end
