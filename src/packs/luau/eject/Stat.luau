--!strict
-- Stat, ejected: one value that matters, drawn with primitives.
-- `local Stat = require("@self/ui/Stat")` then `Stat { value = out, caption = "USD → INR" }`.
local ui = require("@echo/ui")

return function(p: { [string]: any })
	local value, state, err = ui.resolve(p.value)
	if state == "pending" then
		return ui.col({ gap = "sm", padding = "xl", align = "center" }, { ui.box({ width = 120, height = 34, radius = "card", fill = "card", intent = "loading" }, {}) })
	elseif state == "error" then
		return ui.Empty { title = "Couldn't Load", hint = tostring(err), state = "error" }
	end
	local shown = value
	if type(value) == "table" then
		shown = if value.amount ~= nil then `{value.amount} {value.currency}` elseif value.value ~= nil then `{value.value} {value.unit or ""}` else "—"
	end
	return ui.col({ gap = "xs", padding = "xl", align = "center", intent = p.intent }, {
		ui.text(shown, { role = "display" }),
		ui.text(p.caption or "", { role = "caption", color = "text-secondary" }),
	})
end
