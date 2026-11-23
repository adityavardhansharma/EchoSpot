--!strict
-- Chart, ejected: a series, with its title, and its last value beside it.
-- `local Chart = require("@self/ui/Chart")` then `Chart { series = points, kind = "line", title = "Builds" }`.
-- The plot itself is the `chart` primitive; what is around it is yours.
local ui = require("@echo/ui")

return function(p: { [string]: any })
	local series, st, err = ui.resolve(p.series)
	if st == "pending" then
		return ui.box({ height = 110, radius = "card", fill = "card", intent = "loading" }, {})
	elseif st == "error" then
		return ui.Empty { title = "Couldn't Load", hint = tostring(err), state = "error" }
	elseif #(series or {}) == 0 then
		return ui.Empty { title = p.empty or "No Data Yet", icon = "hash" }
	end
	local last = series[#series]
	if type(last) == "table" then
		last = last.v
	end
	return ui.col({ gap = "sm", intent = p.intent }, {
		ui.row({ gap = "md", grow = true }, {
			ui.text(p.title or "", { role = "caption", color = "text-secondary" }),
			ui.spacer(),
			ui.text(if last ~= nil then tostring(last) else "", { role = "tabular" }),
		}),
		ui.chart({ series = series, kind = p.kind or "line", tint = p.tint }),
	})
end
