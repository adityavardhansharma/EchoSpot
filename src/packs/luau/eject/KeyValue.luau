--!strict
-- KeyValue, ejected: labels and their values, drawn with primitives.
local ui = require("@echo/ui")

return function(p: { [string]: any })
	local items = ui.resolve(p.items) or {}
	return ui.col({ padding = "xl" }, ui.each(items, function(item)
		return ui.row({ py = "md", gap = "xl", align = "center" }, {
			ui.text(item[1] or item.label, { role = "caption", color = "text-secondary" }),
			ui.spacer(),
			ui.text(item[2] or item.value, { role = "body" }),
		})
	end))
end
