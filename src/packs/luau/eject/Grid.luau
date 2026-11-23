--!strict
-- Grid, ejected: cells in columns, drawn with primitives.
-- `local Grid = require("@self/ui/Grid")` then `Grid { items = photos, image = "path", title = "name", columns = 4 }`.
--
-- What is yours now: a cell answers a press with the first action's run, or `press(item, cx)`.
local ui = require("@echo/ui")

local function state(p: { [string]: any }): any
	local items, st, err = ui.resolve(p.items)
	if st == "pending" then
		return ui.row({ gap = "md", grow = true }, ui.each({ 1, 2, 3, 4 }, function()
			return ui.box({ height = 96, grow = 1, radius = "card", fill = "card", intent = "loading" }, {})
		end))
	elseif st == "error" then
		return ui.Empty { title = "Couldn't Load", hint = tostring(err), state = "error" }
	elseif #(items or {}) == 0 then
		local empty = if type(p.empty) == "string" then { title = p.empty } else p.empty
		return ui.Empty(empty or { title = "No Results" })
	end
	return items
end

return function(p: { [string]: any })
	local items = state(p)
	if items._ui then
		return items
	end
	local columns = math.clamp(p.columns or 4, 1, 8)
	local lines = {}
	for start = 1, #items, columns do
		local cells = {}
		for i = start, start + columns - 1 do
			local item = items[i]
			if item == nil then
				-- Empty cells keep the last line's cells the same width as the rest.
				table.insert(cells, ui.col({ grow = 1 }, {}))
				continue
			end
			local actions = if type(p.actions) == "function" then p.actions(item, i) else p.actions
			local first = actions and actions[1]
			local run = p.press or (first and first.run)
			local image = ui.pick(item, p.image, i)
			local face = if image
				then ui.image(image, { height = 96, cover = true })
				else ui.box({ height = 96, align = "center", radius = "card", fill = "card" }, { ui.icon(ui.pick(item, p.icon, i) or "circle", { size = "lg", tint = ui.pick(item, p.tint, i) }) })
			table.insert(cells, ui.col({
				gap = "xs", grow = 1, radius = "card", intent = ui.pick(item, p.intent, i),
				press = if run then function(_, cx) run(item, cx) end else nil,
			}, {
				face,
				ui.text(ui.pick(item, p.title, i) or (if type(item) == "table" then item.title or item.name else tostring(item)) or "", { role = "caption", color = "text-secondary" }),
			}))
		end
		table.insert(lines, ui.row({ gap = "md", grow = true }, cells))
	end
	return ui.col({ gap = "md" }, lines)
end
