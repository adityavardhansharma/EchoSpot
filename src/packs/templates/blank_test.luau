return {
	["greets by name"] = function(t)
		t:equal(t:call("hello", { name = "Ada" }), "Hello, Ada.")
	end,
	["greets the world when no name is given"] = function(t)
		t:equal(t:call("hello", {}), "Hello, world.")
	end,
}
