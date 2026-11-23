--!strict
-- Open pull requests, with the failing ones marked urgent. Note what is absent: no loading
-- code, no empty-state drawing, no "open URL" action (the url field brings its verbs), no styling.
local echo = require("@echo")
local t = require("@echo/types")
local ui = require("@echo/ui")

local pr = t.record("pr", { title = t.text(), url = t.url(), repo = t.text(), failing = t.bool() })

return echo.pack {
	abilities = {
		prs = echo.ability {
			title = "My Pull Requests",
			icon = "icons/prs.svg",
			kind = "read",
			input = { repo = t.text():optional() },
			output = t.list(pr),
			refresh = "5m",
			run = function(input, cx)
				local res = cx:call("http.get", {
					url = "https://api.github.com/search/issues",
					query = { q = "is:pr is:open author:@me" .. (if input.repo then " repo:" .. input.repo else "") },
					auth = "github",
				})
				return echo.map(res.json.items, function(i)
					return { title = i.title, url = i.html_url, repo = string.match(i.repository_url, "repos/(.+)$") or "", failing = false }
				end)
			end,
			faces = {
				panel = function(view, cx)
					return ui.List {
						items = cx:load("__ID__.prs", view.args),
						title = "title",
						subtitle = "repo",
						intent = function(p)
							return if p.failing then "urgent" else nil
						end,
						empty = { title = "No Open Pull Requests", hint = "Everything you opened has been merged or closed" },
					}
				end,
			},
		},
	},
}
