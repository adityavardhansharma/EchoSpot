--!strict
-- Table, ejected: rows and columns, with a header line.
-- `local Table = require("@self/ui/Table")` then `Table { columns = { "Repo", "Stars" }, rows = { { "rune", 12 } } }`.
-- A row may also be a record: columns then name its keys (`{ key = "stars", title = "Stars" }`).
local ui = require("@echo/ui")

local function title(column: any): string
	return if type(column) == "table" then tostring(column.title or column.key or "") else tostring(column)
end

local function cell(row: any, column: any, i: number): string
	if row[1] ~= nil then
		local v = row[i]
		return if v == nil then "" else tostring(v)
	end
	local key = if type(column) == "table" then column.key else column
	local v = row[key]
	return if v == nil then "" else tostring(v)
end

return function(p: { [string]: any })
	local source = p.rows
	local columns = p.columns
	if p.value ~= nil then
		local value = ui.resolve(p.value)
		source = value and value.rows
		columns = value and value.columns
	end
	local rows, st, err = ui.resolve(source)
	if st == "pending" then
		return ui.col({ gap = "sm" }, ui.each({ 1, 2, 3, 4 }, function()
			return ui.box({ height = 24, radius = "thumb", fill = "card", intent = "loading", padding = "none" }, {})
		end))
	elseif st == "error" then
		return ui.Empty { title = "Couldn't Load", hint = tostring(err), state = "error" }
	elseif #(rows or {}) == 0 then
		return ui.Empty { title = p.empty or "No Rows", icon = "grid" }
	end
	columns = columns or {}
	local lines = {
		ui.row({ gap = "lg", py = "sm", grow = true }, ui.each(columns, function(column)
			return ui.col({ grow = 1 }, { ui.text(title(column), { role = "caption", color = "text-secondary" }) })
		end)),
		ui.box({ height = 1, fill = "separator", padding = "none", radius = "thumb" }, {}),
	}
	for _, row in rows do
		table.insert(lines, ui.row({ gap = "lg", py = "sm", grow = true }, ui.each(columns, function(column, i)
			return ui.col({ grow = 1 }, { ui.text(cell(row, column, i), { role = "tabular" }) })
		end)))
	end
	return ui.col({ gap = "none" }, lines)
end
