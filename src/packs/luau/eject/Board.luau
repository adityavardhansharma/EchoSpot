--!strict
-- Board, ejected: tiles on a four-column grid. A tile's size is "<columns>x<rows>".
-- `local Board = require("@self/ui/Board")` then `Board { tiles = { { content = Stat { … }, size = "2x1", title = "Rate" } } }`.
--
-- What is yours now: tiles are laid out in reading order, a line at a time; a tile that doesn't
-- fit what is left of a line starts the next one.
local ui = require("@echo/ui")

local COLUMNS = 4
local ROW = 96

local function size(tile: any): (number, number)
	local w, h = string.match(tostring(tile.size or "1x1"), "^(%d)x(%d)$")
	return math.clamp(tonumber(w) or 1, 1, COLUMNS), math.clamp(tonumber(h) or 1, 1, 4)
end

local function card(tile: any, w: number, h: number): any
	return ui.col({
		gap = "sm", padding = "xl", radius = "card", fill = "card", grow = w, height = ROW * h,
		press = if tile.press then function(_, cx) tile.press(cx) end else nil,
	}, {
		if tile.title then ui.text(tile.title, { role = "caption", color = "text-secondary" }) else nil,
		tile.content or ui.Empty { title = "Empty Tile" },
	})
end

return function(p: { [string]: any })
	local lines, line, used = {}, {}, 0
	for _, tile in p.tiles or {} do
		local w, h = size(tile)
		if used + w > COLUMNS then
			table.insert(lines, line)
			line, used = {}, 0
		end
		table.insert(line, card(tile, w, h))
		used += w
	end
	if #line > 0 then
		if used < COLUMNS then
			-- What a short last line leaves stays empty, so its tiles keep their width.
			table.insert(line, ui.col({ grow = COLUMNS - used }, {}))
		end
		table.insert(lines, line)
	end
	if #lines == 0 then
		return ui.Empty { title = "No Tiles", hint = "Add tiles to this board" }
	end
	return ui.col({ gap = "md" }, ui.each(lines, function(l)
		return ui.row({ gap = "md", grow = true }, l)
	end))
end
