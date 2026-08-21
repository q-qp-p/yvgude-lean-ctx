from quality import evaluate


def test_all_required_findings_pass():
    review = {
        "findings": [
            {
                "id": "sql_injection",
                "location": "checkout.py:find_order",
                "summary": "SQL query is concatenated instead of parameterized",
            },
            {
                "id": "missing_authz",
                "location": "checkout.py:export_orders",
                "summary": "Exports orders without an authorization check",
            },
            {
                "id": "discount_off_by_one",
                "location": "checkout.py:apply_discount",
                "summary": "range(len(prices) + 1) indexes past the last price",
            },
        ]
    }
    result = evaluate(review)
    assert result["passed"] is True
    assert result["missing"] == []


def test_missing_defect_fails_closed():
    review = {
        "findings": [
            {
                "id": "sql_injection",
                "location": "checkout.py:find_order",
                "summary": "concatenated SQL",
            }
        ]
    }
    result = evaluate(review)
    assert result["passed"] is False
    assert "missing_authz" in result["missing"]
    assert "discount_off_by_one" in result["missing"]


def test_keyword_match_without_canonical_id():
    review = {
        "findings": [
            {
                "id": "other",
                "location": "checkout.py:find_order",
                "summary": "SQL injection via string concatenation",
            },
            {
                "id": "other",
                "location": "checkout.py:export_orders",
                "summary": "missing authorization before SELECT *",
            },
            {
                "id": "other",
                "location": "checkout.py:apply_discount",
                "summary": "off-by-one in range(len(prices) + 1)",
            },
        ]
    }
    result = evaluate(review)
    assert result["passed"] is True
