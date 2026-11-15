--!strict
-- @echo/types: schemas as data. `t.text()`, `t.list(t.url())`, `t.record("pr", { … })`.
-- A type has three modifiers: `:optional()`, `:default(value)` and `:describe("label")`.

local CORE = ...

local Type = {}
Type.__index = Type

local function make(kind: string, extra: { [string]: any }?)
	local self = { type = kind }
	if extra then
		for k, v in extra do
			self[k] = v
		end
	end
	return setmetatable(self, Type)
end

local function with(self, key: string, value: any)
	local copy = table.clone(self)
	copy[key] = value
	return setmetatable(copy, Type)
end

function Type.optional(self)
	return with(self, "opt", true)
end

function Type.default(self, value: any)
	return with(self, "def", value)
end

function Type.describe(self, label: string)
	return with(self, "label", label)
end

local t = {}

for _, kind in CORE do
	t[kind] = function()
		return make(kind)
	end
end

function t.list(of: any)
	return make("list", { of = of })
end

function t.series()
	return make("series")
end

-- `t.record("pr", { title = t.text() })`, or `t.record({ … })` for one with no name.
function t.record(name: any, fields: any)
	if type(name) == "table" then
		name, fields = "", name
	end
	return make("record", { name = name, fields = fields or {} })
end

-- `t.enum({ "open", "closed" })` or `t.enum("open", "closed")`.
function t.enum(first: any, ...: string)
	local values = if type(first) == "table" then first else { first, ... }
	return make("enum", { values = values })
end

function t.optional(of: any)
	return of:optional()
end

-- Whether `v` was made here.
function t.is(v: any): boolean
	return type(v) == "table" and getmetatable(v) == Type
end

-- A type as plain data, for the host. A bare table of fields is a record.
function t.plain(v: any, depth: number?): any
	local d = depth or 0
	if type(v) ~= "table" or d > 16 then
		return nil
	end
	if not t.is(v) then
		local fields = {}
		for k, f in v do
			fields[k] = t.plain(f, d + 1)
		end
		return { type = "record", fields = fields }
	end
	local out: { [string]: any } = { type = v.type, optional = v.opt or false, default = v.def, label = v.label, name = v.name, values = v.values }
	if v.of then
		out.of = t.plain(v.of, d + 1)
	end
	if v.fields then
		local fields = {}
		for k, f in v.fields do
			fields[k] = t.plain(f, d + 1)
		end
		out.fields = fields
	end
	return out
end

return setmetatable(t, {
	__index = function(_, name)
		error(`@echo/types has no type '{name}'. Core types: {table.concat(CORE, ", ")}`, 2)
	end,
})
