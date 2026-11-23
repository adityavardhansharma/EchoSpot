return {
	["the 1000th prime"] = function(t)
		t:equal(t:call("nth", { n = 1000 }), 7919)
	end,
	["the 100,000th prime, past where 32-bit arithmetic overflows"] = function(t)
		t:equal(t:call("nth", { n = 100000 }), 1299709)
	end,
	["the first primes"] = function(t)
		t:equal(t:call("primes", { limit = 10 }), { 2, 3, 5, 7 })
	end,
	["a bad input says what is wrong"] = function(t)
		local ok, err = t:try("nth", { n = 0 })
		t:ok(not ok, "n = 0 is refused")
		t:match(err.message, "whole number")
	end,
}
