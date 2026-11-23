--!strict
-- Empty, ejected: nothing here, and what to do about it.
local ui = require("@echo/ui")

return function(p: { [string]: any })
	return ui.col({ gap = "xs", padding = "xxl", align = "center", justify = "center", grow = 1 }, {
		ui.icon(p.icon or "circle", { color = "text-tertiary" }),
		ui.text(p.title or "No Results", { role = "headline", color = "text-secondary" }),
		ui.text(p.hint or "", { role = "caption", color = "text-secondary" }),
	})
end
