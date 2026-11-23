--!strict
-- Stream, ejected: text that arrives as it is written, with a mark while it is still coming.
-- `local Stream = require("@self/ui/Stream")` then `Stream { source = cx:load("ai.stream", { prompt = … }) }`.
local ui = require("@echo/ui")

return function(p: { [string]: any })
	local value, st, err = ui.resolve(p.source)
	local text = if type(value) == "table" then value.text else value
	if st == "error" then
		return ui.Empty { title = "Couldn't Finish", hint = tostring(err), state = "error" }
	elseif (text == nil or text == "") and st == "pending" then
		return ui.col({ gap = "sm" }, ui.each({ 1, 2, 3 }, function()
			return ui.box({ height = 14, radius = "thumb", fill = "card", intent = "loading", padding = "none" }, {})
		end))
	end
	return ui.col({ gap = "sm" }, {
		ui.markdown(text or ""),
		if st == "pending" then ui.text("Writing…", { role = "caption", color = "text-secondary", intent = "live" }) else nil,
	})
end
