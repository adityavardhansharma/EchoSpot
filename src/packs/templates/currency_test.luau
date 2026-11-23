return {
	["converts between two currencies"] = function(t)
		t:mock("calc.convert", { amount = 4170.5, currency = "INR" })
		local out = t:call("convert", { amount = 50, from = "usd", to = "inr" })
		t:equal(out.currency, "INR")
		t:equal(out.amount, 4170.5)
	end,
	["says what is wrong with an amount that isn't one"] = function(t)
		local ok, err = t:try("convert", { amount = "fifty", from = "usd", to = "inr" })
		t:equal(ok, false)
		t:equal(err.code, "E0301")
	end,
}
