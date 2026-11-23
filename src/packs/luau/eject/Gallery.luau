--!strict
-- Gallery, ejected: pictures, large, three to a line (one when narrow).
-- `local Gallery = require("@self/ui/Gallery")` then `Gallery { items = shots, image = "path", title = "name" }`.
local ui = require("@echo/ui")

return function(p: { [string]: any })
	local items, st, err = ui.resolve(p.items)
	if st == "pending" then
		return ui.row({ gap = "md", grow = true }, ui.each({ 1, 2, 3 }, function()
			return ui.box({ height = 140, grow = 1, radius = "card", fill = "card", intent = "loading" }, {})
		end))
	elseif st == "error" then
		return ui.Empty { title = "Couldn't Load", hint = tostring(err), state = "error" }
	elseif #(items or {}) == 0 then
		local empty = if type(p.empty) == "string" then { title = p.empty } else p.empty
		return ui.Empty(empty or { title = "No Pictures", icon = "image" })
	end
	local columns = p.columns or 3
	local lines = {}
	for start = 1, #items, columns do
		local cells = {}
		for i = start, start + columns - 1 do
			local item = items[i]
			if item == nil then
				table.insert(cells, ui.col({ grow = 1 }, {}))
				continue
			end
			local actions = if type(p.actions) == "function" then p.actions(item, i) else p.actions
			local first = actions and actions[1]
			local run = p.press or (first and first.run)
			table.insert(cells, ui.col({ gap = "xs", grow = 1, press = if run then function(_, cx) run(item, cx) end else nil }, {
				ui.image(ui.pick(item, p.image, i) or "", { height = 140, cover = true }),
				ui.text(ui.pick(item, p.title, i) or "", { role = "caption", color = "text-secondary" }),
			}))
		end
		table.insert(lines, ui.row({ gap = "md", grow = true }, cells))
	end
	return ui.col({ gap = "lg" }, lines)
end
