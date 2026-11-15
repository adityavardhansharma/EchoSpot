--!strict
-- @echo/ui: say what, not how. Every function here returns a block: plain data naming a
-- component and what it holds. The host owns spacing, type, motion, focus and effects, so a
-- surface built from these looks like it shipped with EchoSpot.
--
-- Three layers, all public:
--   primitives   row col box text spacer each icon image input fx glass markdown code chart
--   components   List Grid Detail Form Stat Chart Table Timeline Gallery KeyValue Progress
--                Banner Empty Tabs Split Stream Board
--   modifiers    look intent density accessory fx empty error (props on any component)
--
-- A component takes a Resource (what `cx:load` returns) wherever it takes data: while it is
-- pending the host shows a skeleton, an error shows the reason with Retry.

local host = ...

local ui = {}

type Block = { [string]: any }

local function block(name: string, props: any): Block
	local b: Block = if type(props) == "table" then table.clone(props) else {}
	b._ui = name
	return b
end

-- A Resource's value and state, or a plain value as it is.
local function resolve(v: any): (any, string, any, any)
	if type(v) == "table" and v._resource then
		return v.value, v.state, v.error, v.type
	end
	return v, "ok", nil, nil
end
ui.resolve = resolve

-- What `spec` says about `item`: a function of the item, a field's name, or a constant.
local function pick(item: any, spec: any, index: number): any
	if spec == nil then
		return nil
	elseif type(spec) == "function" then
		return spec(item, index)
	elseif type(spec) == "string" and type(item) == "table" then
		return item[spec]
	end
	return nil
end
ui.pick = pick

-- Puts a Resource's state on a block, and returns the value to build from.
local function settle(b: Block, source: any): any
	local value, state, err, kind = resolve(source)
	b.state = state
	if err ~= nil then
		b.error = err
	end
	if kind ~= nil and b.type == nil then
		b.type = kind
	end
	return value
end

---- Primitives ----------------------------------------------------------------------------

for _, name in { "row", "col", "box", "glass" } do
	ui[name] = function(props: any, children: any): Block
		-- `ui.col({ a, b })`: children alone.
		if children == nil and type(props) == "table" and props[1] ~= nil then
			props, children = nil, props
		end
		local b = block(name, props)
		local kids = {}
		for _, child in children or {} do
			if child then
				table.insert(kids, if type(child) == "string" then { _ui = "text", text = child } else child)
			end
		end
		b.children = kids
		return b
	end
end

function ui.text(content: any, props: any): Block
	local b = block("text", props)
	b.text = if content == nil then "" else tostring(content)
	return b
end

function ui.spacer(props: any): Block
	return block("spacer", props)
end

function ui.icon(name: string, props: any): Block
	local b = block("icon", props)
	b.name = name
	return b
end

function ui.image(source: string, props: any): Block
	local b = block("image", props)
	b.source = source
	return b
end

function ui.input(props: any): Block
	return block("input", props)
end

function ui.markdown(source: any, props: any): Block
	local b = block("markdown", props)
	b.source = settle(b, source)
	return b
end

function ui.code(source: string, props: any): Block
	local b = block("code", props)
	b.source = source
	return b
end

function ui.chart(props: any): Block
	local b = block("chart", props)
	b.series = settle(b, b.series)
	return b
end

-- `ui.fx({ fire = "heat" }, child)`: a raw effect around a block. The host checks the
-- parameters against rune-fx's catalogue and counts it against the surface's effect budget.
function ui.fx(effects: any, child: any): Block
	return { _ui = "fx", effects = effects, child = child }
end

-- `ui.each(items, function(item, i) return ui.text(item.name) end)`: a list of children.
function ui.each(items: any, build: (any, number) -> any): { any }
	local out = {}
	for i, item in resolve(items) or {} do
		local child = build(item, i)
		if child then
			table.insert(out, child)
		end
	end
	return out
end

---- Components ----------------------------------------------------------------------------

-- The rows of a List, a Grid, a Gallery or a Timeline. Each names what it shows by field
-- (`title = "name"`) or by function (`title = function(item) … end`).
local ROW_FIELDS = { "title", "subtitle", "icon", "image", "accessory", "intent", "section", "time", "keywords", "badge", "tint" }

local function rows(b: Block, p: any): { any }
	local items = settle(b, p.items)
	local out = {}
	for i, item in items or {} do
		local r: Block = { value = item }
		for _, field in ROW_FIELDS do
			r[field] = pick(item, p[field], i)
		end
		if r.title == nil then
			r.title = if type(item) == "table" then item.title or item.name or item.id else tostring(item)
		end
		local id = pick(item, p.id or "id", i)
		r.id = if id ~= nil then tostring(id) else tostring(i)
		if type(p.detail) == "function" then
			r.detail = p.detail(item, i)
		end
		if type(p.actions) == "function" then
			r.actions = p.actions(item, i)
		elseif p.actions ~= nil then
			r.actions = p.actions
		end
		out[i] = r
	end
	-- These told the host how to read an item; the rows carry the answers now.
	for _, field in ROW_FIELDS do
		if field ~= "title" or type(p.title) ~= "string" then
			b[field] = nil
		end
	end
	b.heading = if type(p.heading) == "string" then p.heading else nil
	b.items, b.detail, b.actions, b.id, b.title = nil, nil, nil, nil, nil
	b.rows = out
	return out
end

-- A list of rows: `ui.List { items = prs, title = "title", subtitle = "repo", empty = "No open PRs" }`.
function ui.List(p: any): Block
	local b = block("List", p)
	rows(b, p)
	return b
end

-- A grid of cells: `ui.Grid { items = photos, image = "path", title = "name", columns = 4 }`.
function ui.Grid(p: any): Block
	local b = block("Grid", p)
	rows(b, p)
	return b
end

-- Pictures, large: `ui.Gallery { items = shots, image = "path", title = "name" }`.
function ui.Gallery(p: any): Block
	local b = block("Gallery", p)
	rows(b, p)
	return b
end

-- Things in time order: `ui.Timeline { items = events, title = "title", time = "at" }`.
function ui.Timeline(p: any): Block
	local b = block("Timeline", p)
	rows(b, p)
	return b
end

-- A page: `ui.Detail { markdown = text, fields = { { "Repo", repo } }, actions = { … } }`.
function ui.Detail(p: any): Block
	local b = block("Detail", p)
	b.markdown = settle(b, p.markdown)
	return b
end

-- A form from a schema: `ui.Form { fields = { name = t.text() }, submit = function(values, cx) … end }`,
-- or `ui.Form { ability = "notes.create" }` to ask for an ability's input.
function ui.Form(p: any): Block
	return block("Form", p)
end

-- One number that matters: `ui.Stat { value = out, caption = "USD → INR", history = series }`.
function ui.Stat(p: any): Block
	local b = block("Stat", p)
	b.value = settle(b, p.value)
	if p.history ~= nil then
		b.history = resolve(p.history)
	end
	return b
end

-- A chart of a series: `ui.Chart { series = points, kind = "line" }`. Points are numbers, or
-- `{ t = seconds, v = number }`.
function ui.Chart(p: any): Block
	local b = block("Chart", p)
	b.series = settle(b, p.series)
	return b
end

-- Rows and columns: `ui.Table { columns = { "Repo", "Stars" }, rows = { { "rune", 12 } } }`,
-- or `ui.Table { value = tableValue }`.
function ui.Table(p: any): Block
	local b = block("Table", p)
	if p.value ~= nil then
		local value = settle(b, p.value)
		if type(value) == "table" then
			b.columns, b.rows = value.columns, value.rows
		end
		b.value = nil
	else
		b.rows = settle(b, p.rows)
	end
	return b
end

-- Labels and values: `ui.KeyValue { items = { { "Branch", "main" }, { "Status", "green" } } }`.
function ui.KeyValue(p: any): Block
	local b = block("KeyValue", p)
	local items = settle(b, p.items)
	local out = {}
	for k, v in items or {} do
		if type(k) == "number" and type(v) == "table" then
			table.insert(out, { label = tostring(v[1] or v.label or ""), value = v[2] or v.value })
		else
			table.insert(out, { label = tostring(k), value = v })
		end
	end
	if #out > 0 and (items :: any)[1] == nil then
		table.sort(out, function(a, c)
			return a.label < c.label
		end)
	end
	b.items = out
	return b
end

-- How far along: `ui.Progress { value = 0.4, label = "Uploading" }`.
function ui.Progress(p: any): Block
	local b = block("Progress", p)
	b.value = settle(b, p.value)
	return b
end

-- A message across the top: `ui.Banner { title = "Offline", text = "Showing saved data", intent = "muted" }`.
function ui.Banner(p: any): Block
	return block("Banner", p)
end

-- Nothing here: `ui.Empty { title = "No builds", hint = "Push a commit to see one" }`.
function ui.Empty(p: any): Block
	return block("Empty", p)
end

-- `ui.Tabs { tabs = { { title = "Open", content = ui.List { … } }, … } }`
function ui.Tabs(p: any): Block
	return block("Tabs", p)
end

-- Two panes: `ui.Split { left = ui.List { … }, right = ui.Detail { … } }`.
function ui.Split(p: any): Block
	return block("Split", p)
end

-- Text that arrives as it is written: `ui.Stream { source = cx:load("ai.stream", { prompt = … }) }`.
function ui.Stream(p: any): Block
	local b = block("Stream", p)
	local value = settle(b, p.source)
	b.text = if type(value) == "table" then value.text else value
	b.source = nil
	return b
end

-- Tiles on the host's four-column grid: `ui.Board { tiles = { { content = ui.Stat { … }, size = "2x1" } } }`.
function ui.Board(p: any): Block
	return block("Board", p)
end

local NAMES = {}
for name in ui :: any do
	table.insert(NAMES, name)
end
table.sort(NAMES)
ui.names = NAMES

return setmetatable(ui, {
	__index = function(_, name)
		error(host.unknown("ui", tostring(name)), 2)
	end,
})
