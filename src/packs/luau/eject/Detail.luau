--!strict
-- Detail, ejected: a page of Markdown, a picture over it and fields under it.
-- `local Detail = require("@self/ui/Detail")` then `Detail { markdown = text, fields = { { "Repo", repo } } }`.
local ui = require("@echo/ui")

return function(p: { [string]: any })
	local markdown, st, err = ui.resolve(p.markdown)
	if st == "pending" then
		return ui.col({ gap = "sm" }, ui.each({ 1, 2, 3, 4, 5 }, function()
			return ui.box({ height = 14, radius = "thumb", fill = "card", intent = "loading", padding = "none" }, {})
		end))
	elseif st == "error" then
		return ui.Empty { title = "Couldn't Load", hint = tostring(err), state = "error" }
	end
	local parts = {}
	if type(p.image) == "string" and p.image ~= "" then
		table.insert(parts, ui.image(p.image, { cover = true }))
	end
	if markdown ~= nil and markdown ~= "" then
		table.insert(parts, ui.markdown(markdown))
	end
	local fields = p.fields or {}
	if #fields > 0 then
		table.insert(parts, ui.col({ gap = "none" }, ui.each(fields, function(field)
			return ui.row({ py = "sm", gap = "xl", grow = true }, {
				ui.text(field[1] or field.label or "", { role = "caption", color = "text-secondary" }),
				ui.spacer(),
				ui.text(field[2] or field.value or "", { role = "body" }),
			})
		end)))
	end
	return ui.col({ gap = "xl" }, parts)
end
