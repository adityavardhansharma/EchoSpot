return {
	["failing pull requests are urgent"] = function(t)
		local tree = t:tree("prs", "failing")
		t:match(tree, "Fix CI on arm64")
		t:match(tree, "%[urgent%]")
	end,
	["nothing open says so"] = function(t)
		t:mock("__ID__.prs", {})
		t:match(t:tree("prs"), "No Open Pull Requests")
	end,
	["a failure is shown, with its reason"] = function(t)
		t:fail("__ID__.prs", "The server didn't answer")
		t:match(t:tree("prs"), "error")
	end,
}
