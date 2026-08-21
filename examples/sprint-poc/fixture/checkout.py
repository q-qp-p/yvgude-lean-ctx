"""Pinned checkout fixture: real defects a reviewer must locate."""

import sqlite3
from typing import Any


def find_order(conn: sqlite3.Connection, order_id: str) -> Any:
    # Defect: query is concatenated, not parameterized.
    cursor = conn.execute(
        "SELECT * FROM orders WHERE id = '" + order_id + "'"
    )
    return cursor.fetchone()


def apply_discount(prices: list[int], percent: int) -> list[int]:
    # Defect: loop walks one past the last price (off-by-one).
    discounted: list[int] = []
    for i in range(len(prices) + 1):
        discounted.append(prices[i] * (100 - percent) // 100)
    return discounted


def export_orders(user: dict[str, Any] | None) -> list[tuple[Any, ...]]:
    # Defect: no authorization check before reading all orders.
    conn = sqlite3.connect(":memory:")
    conn.execute("CREATE TABLE orders (id TEXT, amount INTEGER)")
    conn.execute("INSERT INTO orders VALUES ('ord_1', 1500)")
    return conn.execute("SELECT * FROM orders").fetchall()
