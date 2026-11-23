--!strict
-- Form, ejected: a field per input, typed into in place, and a button that sends them.
-- `local Form = require("@self/ui/Form")` then
-- `Form { key = "note", fields = { title = t.text(), body = t.markdown():optional() }, order = { "title", "body" }, submit = function(values, cx) … end }`,
-- or `Form { …, ability = "notes.create" }` to send the values to an ability.
--
-- What is yours now: the host's own form, with its keyboard and its checks, no longer shows.
-- Values arrive as typed; numbers and switches are read from the text, and a required field
-- left empty stops the send with a message under the button.
local ui = require("@echo/ui")

-- What has been typed, per form, kept between draws.
local typed: { [string]: { [string]: string } } = {}
local problems: { [string]: string } = {}

-- The fields in the order `order` gives, then any others by name: a Luau table has no order.
local function names(fields: { [string]: any }, order: { string }?): { string }
	local out, seen = {}, {}
	for _, name in order or {} do
		if fields[name] ~= nil and not seen[name] then
			table.insert(out, name)
			seen[name] = true
		end
	end
	local rest = {}
	for name in fields do
		if not seen[name] then
			table.insert(rest, name)
		end
	end
	table.sort(rest)
	for _, name in rest do
		table.insert(out, name)
	end
	return out
end

-- A field's text as the value its type takes.
local function read(kind: string, text: string?): any
	if text == nil or text == "" then
		return nil
	elseif kind == "number" or kind == "percent" or kind == "duration" then
		return tonumber(text)
	elseif kind == "bool" then
		return text == "true" or text == "yes" or text == "1"
	end
	return text
end

return function(p: { [string]: any })
	local key = p.key or p.title or p.ability or "form"
	local fields = p.fields or {}
	typed[key] = typed[key] or {}
	local values = typed[key]

	local function send(cx: any)
		local out = {}
		for _, name in names(fields, p.order) do
			local f = fields[name]
			local kind = (type(f) == "table" and f.type) or "text"
			local value = read(kind, values[name])
			if value == nil and type(f) == "table" and f.def ~= nil then
				value = f.def
			end
			if value == nil and not (type(f) == "table" and f.opt) then
				problems[key] = `{(type(f) == "table" and f.label) or name} is needed`
				return
			end
			out[name] = value
		end
		problems[key] = nil
		if p.ability then
			cx:call(p.ability, out)
		end
		if p.submit then
			p.submit(out, cx)
		end
		typed[key] = {}
	end

	local rows = {}
	for _, name in names(fields, p.order) do
		local f = fields[name]
		local label = (type(f) == "table" and f.label) or name
		local initial = values[name]
		if initial == nil and type(f) == "table" and f.def ~= nil then
			initial = tostring(f.def)
		end
		table.insert(rows, ui.col({ gap = "xs" }, {
			ui.text(label, { role = "caption", color = "text-secondary" }),
			ui.input({
				key = `{key}-{name}`, value = initial, placeholder = label,
				change = function(text: string)
					values[name] = text
				end,
				submit = function(text: string, cx: any)
					values[name] = text
					send(cx)
				end,
			}),
		}))
	end
	table.insert(rows, ui.row({ gap = "md" }, {
		ui.row({ px = "xl", py = "sm", radius = "control", fill = "card", press = function(_, cx) send(cx) end }, {
			ui.text(p.action or "Submit", { role = "headline", color = "text" }),
		}),
		ui.text(problems[key] or "", { role = "caption", color = "text-secondary", intent = if problems[key] then "warning" else nil }),
	}))
	return ui.col({ gap = "lg" }, rows)
end
