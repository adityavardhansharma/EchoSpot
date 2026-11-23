--!strict
-- Tabs, ejected: a line of titles, and the chosen tab's content under it.
-- `local Tabs = require("@self/ui/Tabs")` then
-- `Tabs { tabs = { { title = "Open", content = … } }, selected = view.args.tab or 1, select = function(i, cx) cx:replace(…, { tab = i }) end }`.
--
-- What is yours now: the host no longer keeps which tab is chosen or puts the titles in the
-- header. You hold `selected`; pressing a title calls `select(index, cx)`.
local ui = require("@echo/ui")

return function(p: { [string]: any })
	local tabs = p.tabs or {}
	if #tabs == 0 then
		return ui.Empty { title = "Nothing Here" }
	end
	local selected = math.clamp(p.selected or 1, 1, #tabs)
	local titles = ui.each(tabs, function(tab, i)
		local chosen = i == selected
		return ui.row({
			px = "lg", py = "sm", radius = "control", fill = if chosen then "card" else nil,
			press = if p.select and not chosen then function(_, cx) p.select(i, cx) end else nil,
		}, {
			ui.text(tab.title or `Tab {i}`, { role = if chosen then "headline" else "body", color = if chosen then "text" else "text-secondary" }),
		})
	end)
	return ui.col({ gap = "lg" }, {
		ui.row({ gap = "xs" }, titles),
		tabs[selected].content or ui.Empty { title = "Nothing Here" },
	})
end
