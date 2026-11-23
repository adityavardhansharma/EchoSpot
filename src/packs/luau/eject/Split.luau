--!strict
-- Split, ejected: two panes side by side, a hairline between them.
-- `local Split = require("@self/ui/Split")` then `Split { left = List { … }, right = Detail { … }, ratio = 0.4 }`.
local ui = require("@echo/ui")

return function(p: { [string]: any })
	local ratio = math.clamp(p.ratio or 0.5, 0.2, 0.8)
	return ui.row({ gap = "xl", grow = true }, {
		ui.col({ grow = ratio }, { p.left }),
		ui.box({ width = 1, fill = "separator", padding = "none", radius = "thumb" }, {}),
		ui.col({ grow = 1 - ratio }, { p.right }),
	})
end
