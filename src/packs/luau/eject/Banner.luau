--!strict
-- Banner, ejected: a message across the top, drawn with primitives.
local ui = require("@echo/ui")

return function(p: { [string]: any })
	return ui.row({ gap = "lg", px = "xl", py = "md", radius = "card", fill = "card", align = "center", intent = p.intent }, {
		ui.icon(p.icon or "bell", {}),
		ui.col({ gap = "xxs", grow = 1 }, {
			ui.text(p.title or "", { role = "headline" }),
			ui.text(p.text or "", { role = "caption", color = "text-secondary", wrap = true }),
		}),
	})
end
