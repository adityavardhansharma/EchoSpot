--!strict
-- @echo: declaring a pack, and the context (`cx`) its code runs with.
--
--   return echo.pack {
--       abilities = { convert = echo.ability { … } },
--       verbs     = { shorten = echo.verb { on = "url", … } },
--       stores    = { builds = echo.store { type = t.list(build), keep = "30d" } },
--       surfaces  = { timer = echo.overlay { place = "top-right", render = … } },
--       boards    = { morning = echo.board { tiles = { … } } },
--   }
--
-- Nothing here touches the world. Every effect is `cx:call(address, input)`, which the host's
-- broker checks against the pack's grants, logs, and records how to undo.

local host, types, ui = ...

local echo = {}
local def: any = nil
-- Functions a rendered surface holds (an action's `run`, a form's `submit`), by surface.
local handlers: { [string]: { any } } = {}

type Any = { [string]: any }

local function tagged(kind: string, surface: string?)
	return function(spec: Any): Any
		if type(spec) ~= "table" then
			error(`echo.{kind} takes a table`, 2)
		end
		spec._echo = kind
		spec._surface = surface
		return spec
	end
end

echo.ability = tagged("ability")
echo.verb = tagged("verb")
echo.store = tagged("store")
echo.board = tagged("board")
echo.trigger = tagged("trigger")
echo.panel = tagged("surface", "panel")
echo.window = tagged("surface", "window")
echo.overlay = tagged("surface", "overlay")
echo.widget = tagged("surface", "widget")

function echo.map(list: any, f: (any, number) -> any): { any }
	local out = {}
	for i, v in list or {} do
		out[i] = f(v, i)
	end
	return out
end

function echo.filter(list: any, keep: (any, number) -> boolean): { any }
	local out = {}
	for i, v in list or {} do
		if keep(v, i) then
			table.insert(out, v)
		end
	end
	return out
end

function echo.find(list: any, want: (any, number) -> boolean): any
	for i, v in list or {} do
		if want(v, i) then
			return v
		end
	end
	return nil
end

function echo.keys(map: any): { any }
	local out = {}
	for k in map or {} do
		table.insert(out, k)
	end
	table.sort(out, function(a, b)
		return tostring(a) < tostring(b)
	end)
	return out
end

function echo.sort(list: any, by: any): { any }
	local out = table.clone(list or {})
	table.sort(out, function(a, b)
		if type(by) == "function" then
			return by(a) < by(b)
		elseif by ~= nil then
			return a[by] < b[by]
		end
		return a < b
	end)
	return out
end

echo.json = { encode = host.json_encode, decode = host.json_decode }
-- Seconds since 1970, and the same as local text: `echo.date("%H:%M")`.
echo.now = host.now
echo.date = host.date
echo.api = host.api

---- Plain data for the host ---------------------------------------------------------------

-- A value as the host can hold it: functions become handles, types become schemas.
local function plain(v: any, reg: { any }?, depth: number): any
	local kind = type(v)
	if kind == "function" then
		if not reg then
			return true
		end
		table.insert(reg, v)
		return { _handler = #reg }
	elseif kind ~= "table" then
		return if kind == "userdata" or kind == "thread" then nil else v
	elseif depth > 48 then
		return nil
	elseif types.is(v) then
		return types.plain(v)
	end
	local out = {}
	for k, x in v do
		if type(k) == "string" or type(k) == "number" then
			out[k] = plain(x, reg, depth + 1)
		end
	end
	return out
end

local function fail(err: any): any
	if type(err) == "table" then
		return err
	end
	return { message = tostring(err), traceback = debug.traceback(nil, 2) }
end

---- cx ------------------------------------------------------------------------------------

-- The context of one entry into the pack. `token` names the call to the broker; `target`
-- names the surface being drawn, when there is one.
local function context(token: number, target: string?): Any
	local cx: Any = { pack = host.id, data = host.data_dir, token = token }

	-- Calls an ability and waits for its result. Raises its error.
	function cx.call(_, address: string, input: any): any
		local r = host.call(token, address, plain(input or {}, nil, 0))
		if r.ok then
			return r.value
		end
		error(r.error, 2)
	end

	-- Calls an ability without raising: `local ok, value_or_error = cx:try(address, input)`.
	function cx.try(_, address: string, input: any): (boolean, any)
		local r = host.call(token, address, plain(input or {}, nil, 0))
		if r.ok then
			return true, r.value
		end
		return false, r.error
	end

	-- A Resource for a surface to draw: pending, ok or error. It never waits; the surface is
	-- drawn again when the result lands. Results are cached per address and input.
	function cx.load(_, address: string, input: any): Any
		return host.load(token, target or "", address, plain(input or {}, nil, 0))
	end

	-- A store's value now. Reading one in a surface draws the surface again when it changes.
	function cx.store(_, name: string): any
		return host.store_get(token, target or "", name, nil)
	end

	-- A store's value as it was at `time` (seconds since 1970).
	function cx.storeAt(_, name: string, time: number): any
		return host.store_get(token, target or "", name, time)
	end

	-- Writes one of this pack's own stores.
	function cx.put(_, name: string, value: any)
		host.store_put(token, name, plain(value, nil, 0))
	end

	function cx.push(_, address: string, args: any)
		host.nav(token, "push", address, plain(args or {}, nil, 0))
	end

	function cx.replace(_, address: string, args: any)
		host.nav(token, "replace", address, plain(args or {}, nil, 0))
	end

	function cx.pop(_)
		host.nav(token, "pop", "", {})
	end

	function cx.toast(_, text: string, intent: string?)
		host.nav(token, "toast", tostring(text), { intent = intent })
	end

	function cx.hud(_, text: string)
		host.nav(token, "hud", tostring(text), {})
	end

	-- True while `echospot dev run --dry` or a test is stubbing effects.
	function cx.dry(_): boolean
		return host.is_dry(token)
	end

	function cx.log(_, ...: any)
		print(...)
	end

	return cx
end

---- The pack ------------------------------------------------------------------------------

local function section(name: string): Any
	local s = def and def[name]
	return if type(s) == "table" then s else {}
end

local function describe_ability(a: Any): Any
	local faces = {}
	for name in a.faces or {} do
		table.insert(faces, name)
	end
	table.sort(faces)
	local on = a.on
	if type(on) == "string" then
		on = { on }
	end
	return {
		title = a.title, description = a.description, icon = a.icon, tint = a.tint, kind = a.kind or "read",
		input = if a.input ~= nil then types.plain(a.input) else nil,
		output = if a.output ~= nil then types.plain(a.output) else nil,
		examples = a.examples, ai = a.ai, slots = a.slots, confirm = a.confirm, hidden = a.hidden, keywords = a.keywords,
		every = a.every, at = a.at, on = on, refresh = a.refresh, hotkey = a.hotkey,
		has_run = type(a.run) == "function", has_query = type(a.query) == "function", has_undo = type(a.undo) == "function",
		faces = faces,
	}
end

local function describe_tiles(tiles: any): { any }
	local out = {}
	for i, tile in tiles or {} do
		if type(tile) == "string" then
			out[i] = { address = tile }
		else
			out[i] = { address = tile[1] or tile.address, args = plain(tile.args, nil, 0), size = tile.size, refresh = tile.refresh, title = tile.title, face = tile.face }
		end
	end
	return out
end

local function describe(): Any
	local out: Any = { title = def.title, look = def.look, abilities = {}, verbs = {}, stores = {}, surfaces = {}, boards = {}, triggers = {} }
	for name, a in section("abilities") do
		out.abilities[name] = describe_ability(a)
	end
	for name, v in section("verbs") do
		out.verbs[name] = { on = v.on, title = v.title, icon = v.icon, kind = v.kind or "effect", has_undo = type(v.undo) == "function" }
	end
	for name, s in section("stores") do
		out.stores[name] = { type = if s.type ~= nil then types.plain(s.type) else nil, keep = s.keep, persist = s.persist or false, initial = plain(s.initial, nil, 0) }
	end
	for name, s in section("surfaces") do
		out.surfaces[name] = {
			surface = s._surface or "panel", title = s.title, icon = s.icon, place = s.place, input = s.input, size = s.size, look = s.look,
			refresh = s.refresh, has_render = type(s.render) == "function",
		}
	end
	for name, b in section("boards") do
		out.boards[name] = { title = b.title, look = b.look, icon = b.icon, tiles = describe_tiles(b.tiles) }
	end
	for name, tr in section("triggers") do
		local on = tr.on
		if type(on) == "string" then
			on = { on }
		end
		out.triggers[name] = { every = tr.every, at = tr.at, on = on, has_run = type(tr.run) == "function" }
	end
	return out
end

-- The view a panel or a surface draws from.
local function view_of(target: string, args: any, state: any): Any
	return { target = target, args = args or {}, query = (state and state.query) or "", size = state and state.size, dark = state and state.dark, tab = state and state.tab }
end

-- What `faces.<name>` and a surface's `render` get as their second argument: `cx`, which also
-- answers to every `@echo/ui` name, so `function(out, ui) return ui.Stat { … } end` works.
local function drawing(token: number, target: string): Any
	local cx = context(token, target)
	return setmetatable(cx, {
		__index = function(_, name)
			return ui[name]
		end,
	})
end

local function finish(id: number, ok: boolean, out: any)
	if ok then
		host.finish(id, true, plain(out, nil, 0))
	else
		host.finish(id, false, plain(fail(out), nil, 0))
	end
end

-- Finds what a target names: `ability:<name>:<face>`, `surface:<name>`, `handler`.
local function parts(target: string): (string, string, string)
	local a, b, c = string.match(target, "^([^:]*):?([^:]*):?(.*)$")
	return a or "", b or "", c or ""
end

function echo.pack(spec: Any): Any
	if type(spec) ~= "table" then
		error("echo.pack takes a table", 2)
	end
	def = spec
	local entry: Any = {}

	entry.__describe = describe

	-- More boards, found in `boards/*.luau` by the host.
	function entry.__boards(found: Any)
		def.boards = def.boards or {}
		for name, board in found do
			if def.boards[name] == nil then
				def.boards[name] = board
			end
		end
	end

	-- Runs an ability's `run(input, cx)` as a task, and reports its result to the broker.
	function entry.__invoke(id: number, name: string, input: any)
		local a = section("abilities")[name]
		if not a or type(a.run) ~= "function" then
			return finish(id, false, { code = "E0300", message = `{host.id}.{name} has no run function` })
		end
		finish(id, pcall(a.run, input, context(id, nil)))
	end

	function entry.__undo(id: number, name: string, input: any, output: any, verb: boolean)
		local a = section(if verb then "verbs" else "abilities")[name]
		if not a or type(a.undo) ~= "function" then
			return finish(id, false, { code = "E0311", message = `{host.id}.{name} has no undo` })
		end
		finish(id, pcall(a.undo, input, output, context(id, nil)))
	end

	function entry.__verb(id: number, name: string, value: any)
		local v = section("verbs")[name]
		if not v or type(v.run) ~= "function" then
			return finish(id, false, { code = "E0300", message = `{host.id} has no verb {name}` })
		end
		finish(id, pcall(v.run, value, context(id, nil)))
	end

	function entry.__trigger(id: number, name: string, event: any)
		local tr = section("triggers")[name]
		if not tr or type(tr.run) ~= "function" then
			return finish(id, false, { code = "E0300", message = `{host.id} has no trigger {name}` })
		end
		finish(id, pcall(tr.run, event, context(id, nil)))
	end

	-- A function a drawn surface holds: an action's `run(item, cx)`, a form's `submit(values, cx)`.
	function entry.__handle(id: number, target: string, n: number, payload: any)
		local f = handlers[target] and handlers[target][n]
		if type(f) ~= "function" then
			return finish(id, false, { code = "E0300", message = "that action is gone; the surface was drawn again" })
		end
		finish(id, pcall(f, payload, context(id, target)))
	end

	-- Draws a target. It can't wait: data comes through `cx:load` and stores.
	function entry.__render(id: number, target: string, args: any, state: any, out: any): any
		local kind, name, face = parts(target)
		local cx = drawing(id, target)
		local view = view_of(target, args, state)
		local built: any
		if kind == "ability" then
			local a = section("abilities")[name]
			local draw = a and a.faces and a.faces[face]
			if type(draw) ~= "function" then
				return nil
			end
			-- A panel draws from the view and loads its own data; every other face draws one output.
			built = if face == "panel" then draw(view, cx) else draw(out, cx, view)
		elseif kind == "surface" then
			local s = section("surfaces")[name]
			if not s or type(s.render) ~= "function" then
				error(`{host.id} has no surface {name}`, 0)
			end
			built = s.render(view, cx)
		else
			error(`can't draw {target}`, 0)
		end
		local reg = {}
		handlers[target] = reg
		return plain(built, reg, 0)
	end

	-- Root search: every ability's `query(text, cx)`, each answer a value or nil.
	function entry.__query(id: number, text: string): { any }
		local found = {}
		for name, a in section("abilities") do
			if type(a.query) == "function" then
				local ok, out = pcall(a.query, text, drawing(id, "query:" .. name))
				if ok and out ~= nil then
					table.insert(found, { ability = name, value = plain(out, nil, 0) })
				elseif not ok then
					warn(`{name}.query: {if type(out) == "table" then out.message else out}`)
				end
			end
		end
		return found
	end

	entry.saveHotState = spec.saveHotState
	entry.loadHotState = spec.loadHotState
	return entry
end

return setmetatable(echo, {
	__index = function(_, name)
		error(host.unknown("echo", tostring(name)), 2)
	end,
})
