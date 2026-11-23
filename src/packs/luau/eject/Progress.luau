--!strict
-- Progress, ejected: how far along, from 0 to 1.
local ui = require("@echo/ui")

return function(p: { [string]: any })
	local value = math.clamp(ui.resolve(p.value) or 0, 0, 1)
	return ui.col({ gap = "sm", intent = p.intent }, {
		ui.row({ gap = "md", grow = true }, { ui.text(p.label or "", { role = "caption", color = "text-secondary" }), ui.spacer(), ui.text(`{math.floor(value * 100)}%`, { role = "caption", color = "text-secondary" }) }),
		ui.row({ height = 6, radius = "thumb", fill = "card", padding = "none", gap = "none", grow = true }, {
			ui.box({ height = 6, width = `{math.max(value * 100, 1)}%`, radius = "thumb", fill = p.tint or "accent", padding = "none" }, {}),
		}),
	})
end
