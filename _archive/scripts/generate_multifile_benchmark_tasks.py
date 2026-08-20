#!/usr/bin/env python3
"""Generate scripts/multifile_benchmark_tasks.json for the multi-file benchmark."""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

PYTHON_BIN = "/opt/homebrew/bin/python3.11"
OUTPUT_PATH = Path(__file__).parent / "multifile_benchmark_tasks.json"

# Reference implementations (used only for --validate, not shipped to LLM).
REFERENCES = {}

TASKS = [
    {
        "id": "math-utils",
        "complexity": "simple",
        "description": "Material cost from shape formulas and unit conversion",
        "files": {
            "formulas.py": '''import math

def circle_area(radius_m: float) -> float:
    return math.pi * radius_m ** 2

def rectangle_area(width_m: float, height_m: float) -> float:
    return width_m * height_m

def cylinder_volume(radius_m: float, height_m: float) -> float:
    return math.pi * radius_m ** 2 * height_m

def box_volume(width_m: float, height_m: float, depth_m: float) -> float:
    return width_m * height_m * depth_m

SHAPE_CALCULATORS = {
    "circle": ("area", lambda dims: circle_area(dims["radius"])),
    "rectangle": ("area", lambda dims: rectangle_area(dims["width"], dims["height"])),
    "cylinder": ("volume", lambda dims: cylinder_volume(dims["radius"], dims["height"])),
    "box": ("volume", lambda dims: box_volume(dims["width"], dims["height"], dims["depth"])),
}
''',
            "converter.py": '''def cm_to_m(value: float) -> float:
    return value / 100.0

def mm_to_m(value: float) -> float:
    return value / 1000.0

DIMENSION_CONVERTERS = {
    "cm": cm_to_m,
    "mm": mm_to_m,
    "m": lambda x: x,
}
''',
        },
        "question": """Write `calculate_material_cost(shape, dimensions, dim_unit, material_density_kg_m3, price_per_kg) -> float` that:
1. Uses DIMENSION_CONVERTERS from converter.py to convert every value in `dimensions` from `dim_unit` to meters
2. Uses SHAPE_CALCULATORS from formulas.py to compute area (m²) or volume (m³)
3. Computes mass = measure * material_density_kg_m3 (density applies to area for 2D shapes and volume for 3D shapes)
4. Returns mass * price_per_kg rounded to 2 decimals

`shape` is one of: circle, rectangle, cylinder, box. `dimensions` keys match the shape (circle/ cylinder: radius+height; rectangle: width+height; box: width+height+depth).""",
        "test": """
cost = calculate_material_cost(
    "cylinder",
    {"radius": 10, "height": 20},
    "cm",
    2700,
    5.0,
)
assert cost == round(3.141592653589793 * 0.1 ** 2 * 0.2 * 2700 * 5.0, 2)

flat = calculate_material_cost(
    "rectangle",
    {"width": 200, "height": 100},
    "cm",
    7800,
    3.0,
)
assert flat == round(2.0 * 1.0 * 7800 * 3.0, 2)

print("ALL TESTS PASSED")
""",
        "reference": '''
def calculate_material_cost(shape, dimensions, dim_unit, material_density_kg_m3, price_per_kg):
    from converter import DIMENSION_CONVERTERS
    from formulas import SHAPE_CALCULATORS

    convert = DIMENSION_CONVERTERS[dim_unit]
    dims_m = {k: convert(v) for k, v in dimensions.items()}
    _, calc = SHAPE_CALCULATORS[shape]
    measure = calc(dims_m)
    mass = measure * material_density_kg_m3
    return round(mass * price_per_kg, 2)
''',
    },
    {
        "id": "auth-basic",
        "complexity": "simple",
        "description": "Authenticate user and return token",
        "files": {
            "users.py": '''import hashlib

class User:
    def __init__(self, user_id: int, username: str, password_hash: str, role: str = "user"):
        self.user_id = user_id
        self.username = username
        self.password_hash = password_hash
        self.role = role
        self.is_active = True

    @staticmethod
    def hash_password(password: str) -> str:
        return hashlib.sha256(password.encode()).hexdigest()

    def check_password(self, password: str) -> bool:
        return self.hash_password(password) == self.password_hash
''',
            "tokens.py": '''import base64
import hashlib
import json
import time

DEFAULT_SECRET = "lean-ctx-secret"
DEFAULT_TTL = 3600

def create_token(user_id: int, username: str, role: str, secret: str = DEFAULT_SECRET, ttl: int = DEFAULT_TTL) -> str:
    payload = {
        "user_id": user_id,
        "username": username,
        "role": role,
        "exp": int(time.time()) + ttl,
    }
    data = base64.urlsafe_b64encode(json.dumps(payload).encode()).decode()
    sig = hashlib.sha256((data + secret).encode()).hexdigest()[:16]
    return f"{data}.{sig}"
''',
        },
        "question": """Write `authenticate(username, password, users_db) -> str` that:
1. Looks up `username` in `users_db` (dict mapping username -> User)
2. Returns None if user missing, inactive, or password wrong (use User.check_password)
3. Otherwise returns a token from create_token(user.user_id, user.username, user.role)

Use User from users.py and create_token from tokens.py.""",
        "test": """
from users import User

alice = User(1, "alice", User.hash_password("secret123"), role="admin")
bob = User(2, "bob", User.hash_password("bobpass"))
bob.is_active = False
db = {"alice": alice, "bob": bob}

token = authenticate("alice", "secret123", db)
assert token is not None
assert isinstance(token, str) and "." in token

assert authenticate("alice", "wrong", db) is None
assert authenticate("bob", "bobpass", db) is None
assert authenticate("missing", "x", db) is None

print("ALL TESTS PASSED")
""",
        "reference": '''
def authenticate(username, password, users_db):
    from tokens import create_token

    user = users_db.get(username)
    if user is None or not user.is_active or not user.check_password(password):
        return None
    return create_token(user.user_id, user.username, user.role)
''',
    },
    {
        "id": "logger-filter",
        "complexity": "simple",
        "description": "Create logger with level and pattern filters",
        "files": {
            "logger.py": '''from typing import List

LEVELS = {"DEBUG": 10, "INFO": 20, "WARNING": 30, "ERROR": 40}

class Logger:
    def __init__(self, name: str, min_level: int = 10):
        self.name = name
        self.min_level = min_level
        self.messages: List[str] = []

    def log(self, level: str, message: str) -> bool:
        if LEVELS[level] < self.min_level:
            return False
        entry = f"[{level}] {self.name}: {message}"
        self.messages.append(entry)
        return True
''',
            "filters.py": '''from typing import List

def matches_any_pattern(text: str, patterns: List[str]) -> bool:
    lowered = text.lower()
    return any(p.lower() in lowered for p in patterns)

def should_log(message: str, exclude_patterns: List[str]) -> bool:
    if not exclude_patterns:
        return True
    return not matches_any_pattern(message, exclude_patterns)
''',
        },
        "question": """Write `create_filtered_logger(name, min_level, exclude_patterns) -> Logger` that returns a Logger subclass or wrapper whose `.log(level, message)`:
1. Delegates to Logger.log only if should_log(message, exclude_patterns) from filters.py is True
2. Uses LEVELS from logger.py to interpret `min_level` when given as a string (e.g. "INFO") or int
3. Returns the same bool Logger.log would return when a line is recorded

Do not mutate the base Logger class globally.""",
        "test": """
log = create_filtered_logger("app", "INFO", ["password", "secret"])
assert log.log("DEBUG", "debug msg") is False
assert log.log("INFO", "hello world") is True
assert log.log("ERROR", "bad password leak") is False
assert log.log("WARNING", "disk full") is True
assert log.messages == ["[INFO] app: hello world", "[WARNING] app: disk full"]

log2 = create_filtered_logger("svc", 10, [])
assert log2.log("DEBUG", "trace") is True
assert len(log2.messages) == 1

print("ALL TESTS PASSED")
""",
        "reference": '''
def create_filtered_logger(name, min_level, exclude_patterns):
    from filters import should_log
    from logger import LEVELS, Logger

    level_value = LEVELS[min_level] if isinstance(min_level, str) else min_level
    base = Logger(name, level_value)

    class FilteredLogger(Logger):
        def log(self, level, message):
            if not should_log(message, exclude_patterns):
                return False
            return super().log(level, message)

    filtered = FilteredLogger(name, level_value)
    filtered.messages = base.messages
    return filtered
''',
    },
    {
        "id": "inventory-basic",
        "complexity": "simple",
        "description": "Process warehouse order into receipt",
        "files": {
            "products.py": '''from dataclasses import dataclass

@dataclass
class Product:
    sku: str
    name: str
    price: float
''',
            "warehouse.py": '''from typing import Dict

class Warehouse:
    def __init__(self, stock: Dict[str, int]):
        self.stock = dict(stock)

    def available(self, sku: str) -> int:
        return self.stock.get(sku, 0)

    def deduct(self, sku: str, qty: int) -> bool:
        if self.available(sku) < qty:
            return False
        self.stock[sku] -= qty
        return True
''',
        },
        "question": """Write `process_order(warehouse, catalog, order_items) -> dict` where:
- `catalog` is dict sku -> Product
- `order_items` is list of {"sku": str, "qty": int}
- For each item: skip unknown sku; if insufficient stock, mark order failed
- On success deduct stock and return {"status": "ok", "items": [{"sku", "name", "qty", "line_total"}], "total": float}
- On any stock failure return {"status": "failed", "items": [], "total": 0.0} without mutating stock

Use Product from products.py and Warehouse from warehouse.py.""",
        "test": """
from products import Product
from warehouse import Warehouse

catalog = {
    "A": Product("A", "Apple", 1.5),
    "B": Product("B", "Banana", 0.75),
}
wh = Warehouse({"A": 5, "B": 2})

receipt = process_order(wh, catalog, [{"sku": "A", "qty": 2}, {"sku": "B", "qty": 1}])
assert receipt["status"] == "ok"
assert receipt["total"] == 3.75
assert wh.available("A") == 3
assert wh.available("B") == 1

fail = process_order(wh, catalog, [{"sku": "B", "qty": 5}])
assert fail["status"] == "failed"
assert wh.available("B") == 1

print("ALL TESTS PASSED")
""",
        "reference": '''
def process_order(warehouse, catalog, order_items):
    lines = []
    total = 0.0
    pending = []
    for item in order_items:
        sku = item["sku"]
        qty = item["qty"]
        product = catalog.get(sku)
        if product is None:
            continue
        if warehouse.available(sku) < qty:
            return {"status": "failed", "items": [], "total": 0.0}
        pending.append((sku, qty, product))
    for sku, qty, product in pending:
        warehouse.deduct(sku, qty)
        line_total = product.price * qty
        total += line_total
        lines.append({"sku": sku, "name": product.name, "qty": qty, "line_total": line_total})
    return {"status": "ok", "items": lines, "total": total}
''',
    },
    {
        "id": "text-pipeline",
        "complexity": "simple",
        "description": "Build text cleaning pipeline from config",
        "files": {
            "transformers.py": '''import re

def upper(text: str) -> str:
    return text.upper()

def strip(text: str) -> str:
    return text.strip()

def normalize(text: str) -> str:
    return re.sub(r"\\s+", " ", strip(text))

TRANSFORMERS = {
    "upper": upper,
    "strip": strip,
    "normalize": normalize,
}
''',
            "pipeline.py": '''from typing import Callable, List

class Pipeline:
    def __init__(self):
        self.steps: List[Callable[[str], str]] = []

    def add(self, fn: Callable[[str], str]) -> "Pipeline":
        self.steps.append(fn)
        return self

    def run(self, text: str) -> str:
        result = text
        for step in self.steps:
            result = step(result)
        return result
''',
        },
        "question": """Write `create_cleaning_pipeline(config_dict) -> Pipeline` that reads `config_dict["steps"]` (list of transformer names) and adds each function from TRANSFORMERS in transformers.py to a new Pipeline from pipeline.py, preserving order.""",
        "test": """
pipe = create_cleaning_pipeline({"steps": ["strip", "normalize", "upper"]})
assert pipe.run("  hello   world  ") == "HELLO WORLD"

empty = create_cleaning_pipeline({"steps": []})
assert empty.run("x") == "x"

print("ALL TESTS PASSED")
""",
        "reference": '''
def create_cleaning_pipeline(config_dict):
    from pipeline import Pipeline
    from transformers import TRANSFORMERS

    pipe = Pipeline()
    for name in config_dict.get("steps", []):
        pipe.add(TRANSFORMERS[name])
    return pipe
''',
    },
    {
        "id": "api-handler",
        "complexity": "medium",
        "description": "Create order using models, validators, and database",
        "files": {
            "models.py": '''from dataclasses import dataclass, field
from typing import List

@dataclass
class User:
    id: int
    name: str
    email: str
    is_active: bool = True

@dataclass
class Product:
    id: int
    name: str
    price: float
    stock: int

@dataclass
class Order:
    id: int
    user_id: int
    items: List[dict] = field(default_factory=list)
    total: float = 0.0
    status: str = "pending"
''',
            "validators.py": '''from typing import Any

class ValidationError(Exception):
    def __init__(self, field: str, message: str):
        self.field = field
        self.message = message
        super().__init__(f"{field}: {message}")

def validate_positive(field: str, value: Any) -> bool:
    if not isinstance(value, (int, float)) or value <= 0:
        raise ValidationError(field, "Must be a positive number")
    return True

def validate_stock(product_stock: int, requested: int) -> bool:
    if requested > product_stock:
        raise ValidationError("quantity", f"Insufficient stock: {product_stock} available, {requested} requested")
    return True
''',
            "database.py": '''from typing import Dict, Optional, List

class InMemoryDB:
    def __init__(self):
        self._users: Dict[int, dict] = {}
        self._products: Dict[int, dict] = {}
        self._orders: Dict[int, dict] = {}
        self._next_id = {"users": 1, "products": 1, "orders": 1}

    def add_user(self, name: str, email: str, is_active: bool = True) -> int:
        uid = self._next_id["users"]
        self._next_id["users"] += 1
        self._users[uid] = {"id": uid, "name": name, "email": email, "is_active": is_active}
        return uid

    def get_user(self, user_id: int) -> Optional[dict]:
        return self._users.get(user_id)

    def add_product(self, name: str, price: float, stock: int) -> int:
        pid = self._next_id["products"]
        self._next_id["products"] += 1
        self._products[pid] = {"id": pid, "name": name, "price": price, "stock": stock}
        return pid

    def get_product(self, product_id: int) -> Optional[dict]:
        return self._products.get(product_id)

    def update_stock(self, product_id: int, delta: int):
        self._products[product_id]["stock"] += delta

    def add_order(self, user_id: int, items: list, total: float) -> int:
        oid = self._next_id["orders"]
        self._next_id["orders"] += 1
        self._orders[oid] = {"id": oid, "user_id": user_id, "items": items, "total": total, "status": "pending"}
        return oid
''',
        },
        "question": """Write `create_order(db, user_id, cart) -> dict` that:
1. Validates user exists and is_active (else ValidationError on field "user")
2. For each cart item {"product_id", "quantity"}: validate_positive on quantity, validate_stock against product stock
3. Computes total using product price * quantity, creates order via db.add_order, decrements stock via db.update_stock
4. Returns {"order_id": int, "total": float, "items_count": int}

Use ValidationError and validators from validators.py; db methods from database.py.""",
        "test": """
from validators import ValidationError

db = InMemoryDB()
uid = db.add_user("Alice", "alice@test.com")
p1 = db.add_product("Widget", 10.0, 50)
p2 = db.add_product("Gadget", 25.0, 10)

result = create_order(db, uid, [{"product_id": p1, "quantity": 2}, {"product_id": p2, "quantity": 1}])
assert result["total"] == 45.0
assert result["items_count"] == 2
assert db.get_product(p1)["stock"] == 48

try:
    create_order(db, 999, [{"product_id": p1, "quantity": 1}])
    assert False
except ValidationError:
    pass

try:
    create_order(db, uid, [{"product_id": p2, "quantity": 100}])
    assert False
except ValidationError:
    pass

print("ALL TESTS PASSED")
""",
        "reference": '''
def create_order(db, user_id, cart):
    from validators import ValidationError, validate_positive, validate_stock

    user = db.get_user(user_id)
    if user is None or not user.get("is_active", False):
        raise ValidationError("user", "Invalid or inactive user")

    total = 0.0
    lines = []
    for item in cart:
        validate_positive("quantity", item["quantity"])
        product = db.get_product(item["product_id"])
        validate_stock(product["stock"], item["quantity"])
        line_total = product["price"] * item["quantity"]
        total += line_total
        lines.append(item)

    order_id = db.add_order(user_id, lines, total)
    for item in cart:
        db.update_stock(item["product_id"], -item["quantity"])

    return {"order_id": order_id, "total": total, "items_count": len(cart)}
''',
    },
    {
        "id": "config-loader",
        "complexity": "medium",
        "description": "Load and merge configuration from multiple sources",
        "files": {
            "config_schema.py": '''from typing import Any, List

SCHEMA = {
    "server": {
        "host": {"type": str, "default": "localhost", "required": False},
        "port": {"type": int, "default": 8080, "required": False},
    },
    "database": {
        "url": {"type": str, "default": None, "required": True},
        "pool_size": {"type": int, "default": 5, "required": False},
    },
}

def validate_config(config: dict, schema: dict = SCHEMA) -> List[str]:
    errors = []
    for section, fields in schema.items():
        for field, rules in fields.items():
            value = config.get(section, {}).get(field)
            if value is None and rules["required"]:
                errors.append(f"{section}.{field} is required")
            elif value is not None and not isinstance(value, rules["type"]):
                errors.append(f"{section}.{field} must be {rules['type'].__name__}")
    return errors

def apply_defaults(config: dict, schema: dict = SCHEMA) -> dict:
    result = {}
    for section, fields in schema.items():
        result[section] = {}
        for field, rules in fields.items():
            value = config.get(section, {}).get(field)
            result[section][field] = value if value is not None else rules["default"]
    return result
''',
            "config_sources.py": '''import json
import os
from typing import Any, Dict

def load_from_file(path: str) -> Dict[str, Any]:
    if not os.path.exists(path):
        return {}
    with open(path) as f:
        return json.load(f)

def load_from_env(prefix: str = "APP") -> Dict[str, Any]:
    config: Dict[str, Any] = {}
    for key, value in os.environ.items():
        if not key.startswith(prefix + "_"):
            continue
        parts = key[len(prefix) + 1 :].lower().split("_", 1)
        if len(parts) != 2:
            continue
        section, field = parts
        if value.lower() in ("true", "false"):
            value = value.lower() == "true"
        else:
            try:
                value = int(value)
            except ValueError:
                pass
        config.setdefault(section, {})[field] = value
    return config

def deep_merge(base: dict, override: dict) -> dict:
    result = dict(base)
    for key, value in override.items():
        if key in result and isinstance(result[key], dict) and isinstance(value, dict):
            result[key] = deep_merge(result[key], value)
        else:
            result[key] = value
    return result
''',
        },
        "question": """Write `load_config(sources) -> dict` where `sources` is an ordered list of dicts describing inputs:
- {"type": "file", "path": str} -> load_from_file
- {"type": "env", "prefix": str} -> load_from_env(prefix)
- {"type": "dict", "data": dict} -> use data directly

Deep-merge each source left-to-right (later wins), then apply_defaults, then validate_config. Raise ValueError with joined errors if invalid. Return final config.""",
        "test": """
import json
import os
import tempfile

with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
    json.dump({"database": {"url": "postgres://localhost/test"}, "server": {"port": 9090}}, f)
    f.flush()
    path = f.name

config = load_config([
    {"type": "file", "path": path},
    {"type": "dict", "data": {"server": {"host": "0.0.0.0"}}},
])
assert config["server"]["port"] == 9090
assert config["server"]["host"] == "0.0.0.0"
assert config["database"]["url"] == "postgres://localhost/test"
assert config["database"]["pool_size"] == 5
os.unlink(path)

os.environ["APP_SERVER_PORT"] = "3000"
os.environ["APP_DATABASE_URL"] = "postgres://prod/db"
config = load_config([{"type": "env", "prefix": "APP"}])
assert config["server"]["port"] == 3000
assert config["database"]["url"] == "postgres://prod/db"
del os.environ["APP_SERVER_PORT"]
del os.environ["APP_DATABASE_URL"]

try:
    load_config([])
    assert False
except ValueError:
    pass

print("ALL TESTS PASSED")
""",
        "reference": '''
def load_config(sources):
    from config_schema import apply_defaults, validate_config
    from config_sources import deep_merge, load_from_env, load_from_file

    merged = {}
    for source in sources:
        stype = source["type"]
        if stype == "file":
            chunk = load_from_file(source["path"])
        elif stype == "env":
            chunk = load_from_env(source.get("prefix", "APP"))
        elif stype == "dict":
            chunk = source["data"]
        else:
            raise ValueError(f"Unknown source type: {stype}")
        merged = deep_merge(merged, chunk)

    config = apply_defaults(merged)
    errors = validate_config(config)
    if errors:
        raise ValueError("; ".join(errors))
    return config
''',
    },
    {
        "id": "event-system",
        "complexity": "medium",
        "description": "Dispatch events to registered handlers",
        "files": {
            "events.py": '''from dataclasses import dataclass, field
from typing import Any, Dict
from datetime import datetime

@dataclass
class Event:
    name: str
    data: Dict[str, Any] = field(default_factory=dict)
    timestamp: datetime = field(default_factory=datetime.now)

@dataclass
class UserCreated(Event):
    name: str = "user.created"

@dataclass
class OrderPlaced(Event):
    name: str = "order.placed"

@dataclass
class PaymentProcessed(Event):
    name: str = "payment.processed"

EVENT_TYPES = {
    "user.created": UserCreated,
    "order.placed": OrderPlaced,
    "payment.processed": PaymentProcessed,
}
''',
            "handlers.py": '''class HandlerResult:
    def __init__(self, handler_name: str, success: bool, message: str = ""):
        self.handler_name = handler_name
        self.success = success
        self.message = message

def send_welcome_email(event) -> HandlerResult:
    email = event.data.get("email", "")
    if not email:
        return HandlerResult("send_welcome_email", False, "No email")
    return HandlerResult("send_welcome_email", True, f"Sent to {email}")

def update_analytics(event) -> HandlerResult:
    return HandlerResult("update_analytics", True, f"Tracked {event.name}")

def check_fraud(event) -> HandlerResult:
    amount = event.data.get("amount", 0)
    if amount > 10000:
        return HandlerResult("check_fraud", False, "Flagged for review")
    return HandlerResult("check_fraud", True, "Passed")

def notify_warehouse(event) -> HandlerResult:
    items = event.data.get("items", [])
    if not items:
        return HandlerResult("notify_warehouse", False, "No items")
    return HandlerResult("notify_warehouse", True, f"Notified for {len(items)} items")

DEFAULT_HANDLERS = {
    "user.created": [send_welcome_email, update_analytics],
    "order.placed": [notify_warehouse, update_analytics],
    "payment.processed": [check_fraud, update_analytics],
}
''',
        },
        "question": """Write `dispatch(event_name, data) -> dict` that:
1. Instantiates the Event subclass from EVENT_TYPES (or base Event if unknown) with name=event_name and data=data
2. Runs handlers from DEFAULT_HANDLERS for that event (empty list if none)
3. Returns {"event": event_name, "handlers_run": int, "all_success": bool, "results": [{"handler", "success", "message"}, ...]}""",
        "test": """
r = dispatch("user.created", {"email": "test@test.com"})
assert r["event"] == "user.created"
assert r["handlers_run"] == 2
assert r["all_success"] is True
assert r["results"][0]["handler"] == "send_welcome_email"

r = dispatch("payment.processed", {"amount": 50000})
assert r["all_success"] is False
fraud = [x for x in r["results"] if x["handler"] == "check_fraud"][0]
assert fraud["success"] is False

r = dispatch("unknown.event", {})
assert r["handlers_run"] == 0
assert r["all_success"] is True

print("ALL TESTS PASSED")
""",
        "reference": '''
def dispatch(event_name, data):
    from events import EVENT_TYPES, Event
    from handlers import DEFAULT_HANDLERS

    cls = EVENT_TYPES.get(event_name, Event)
    if cls is Event:
        event = Event(name=event_name, data=data)
    else:
        event = cls(data=data)

    results = []
    for handler in DEFAULT_HANDLERS.get(event_name, []):
        hr = handler(event)
        results.append({"handler": hr.handler_name, "success": hr.success, "message": hr.message})

    return {
        "event": event_name,
        "handlers_run": len(results),
        "all_success": all(r["success"] for r in results),
        "results": results,
    }
''',
    },
    {
        "id": "cache-layer",
        "complexity": "medium",
        "description": "Cached query with TTL through CacheStore",
        "files": {
            "cache.py": '''import time
from collections import OrderedDict
from typing import Any, Optional

class LRUCache:
    def __init__(self, max_size: int = 128):
        self.max_size = max_size
        self._data: OrderedDict[str, tuple[Any, float]] = OrderedDict()

    def get(self, key: str) -> Optional[Any]:
        if key not in self._data:
            return None
        value, expires = self._data[key]
        if expires and time.time() > expires:
            del self._data[key]
            return None
        self._data.move_to_end(key)
        return value

    def set(self, key: str, value: Any, ttl: Optional[int] = None):
        expires = time.time() + ttl if ttl else 0
        self._data[key] = (value, expires)
        self._data.move_to_end(key)
        while len(self._data) > self.max_size:
            self._data.popitem(last=False)
''',
            "serializer.py": '''import json
from typing import Any

def serialize(value: Any) -> str:
    return json.dumps(value)

def deserialize(raw: str) -> Any:
    return json.loads(raw)
''',
            "store.py": '''from cache import LRUCache
from serializer import deserialize, serialize

class CacheStore:
    def __init__(self, max_size: int = 128):
        self._cache = LRUCache(max_size)

    def get(self, key: str):
        raw = self._cache.get(key)
        return None if raw is None else deserialize(raw)

    def set(self, key: str, value, ttl: int | None = None):
        self._cache.set(key, serialize(value), ttl=ttl)
''',
        },
        "question": """Write `cached_query(store, query_fn, key, ttl) -> Any` that:
1. Returns cached value from CacheStore.get(key) if present
2. Otherwise calls query_fn(), stores result with store.set(key, value, ttl), and returns it

Use CacheStore from store.py.""",
        "test": """
calls = {"n": 0}

def expensive():
    calls["n"] += 1
    return {"answer": 42}

store = CacheStore()
assert cached_query(store, expensive, "k1", ttl=60) == {"answer": 42}
assert calls["n"] == 1
assert cached_query(store, expensive, "k1", ttl=60) == {"answer": 42}
assert calls["n"] == 1

store2 = CacheStore()
assert cached_query(store2, expensive, "k2", ttl=0) == {"answer": 42}
assert calls["n"] == 2

print("ALL TESTS PASSED")
""",
        "reference": '''
def cached_query(store, query_fn, key, ttl):
    hit = store.get(key)
    if hit is not None:
        return hit
    value = query_fn()
    store.set(key, value, ttl=ttl)
    return value
''',
    },
    {
        "id": "http-client",
        "complexity": "medium",
        "description": "Send HTTP request through middleware chain",
        "files": {
            "request.py": '''from dataclasses import dataclass, field
from typing import Dict

@dataclass
class Request:
    method: str
    url: str
    headers: Dict[str, str] = field(default_factory=dict)
    body: str = ""
''',
            "response.py": '''from dataclasses import dataclass, field
from typing import Dict

@dataclass
class Response:
    status: int
    headers: Dict[str, str] = field(default_factory=dict)
    body: str = ""
''',
            "middleware.py": '''from request import Request
from response import Response

def auth_middleware(req: Request, nxt):
    if "Authorization" not in req.headers:
        return Response(status=401, body="unauthorized")
    return nxt(req)

def retry_middleware(req: Request, nxt):
    resp = nxt(req)
    if resp.status >= 500:
        return nxt(req)
    return resp

def logging_middleware(req: Request, nxt):
    resp = nxt(req)
    resp.headers["X-Logged"] = req.method
    return resp

def transport_stub(req: Request) -> Response:
    if not hasattr(transport_stub, "_attempts"):
        transport_stub._attempts = {}
    count = transport_stub._attempts.get(req.url, 0) + 1
    transport_stub._attempts[req.url] = count
    if req.url.endswith("/fail") and count == 1:
        return Response(status=503, body="down")
    if req.url.endswith("/fail"):
        return Response(status=200, body="recovered")
    return Response(status=200, body=f"ok:{req.method}:{req.url}")
''',
        },
        "question": """Write `send_request(request, middlewares) -> Response` that composes middleware functions (each `(Request, next) -> Response`) around `transport_stub` from middleware.py. The first middleware in the list is outermost (runs first). Return the final Response.""",
        "test": """
req = Request("GET", "https://api.example.com/users", headers={"Authorization": "Bearer x"})
resp = send_request(req, [logging_middleware, auth_middleware, retry_middleware])
assert resp.status == 200
assert resp.body == "ok:GET:https://api.example.com/users"
assert resp.headers.get("X-Logged") == "GET"

bad = Request("GET", "https://api.example.com/users")
resp = send_request(bad, [auth_middleware])
assert resp.status == 401

fail_req = Request("POST", "https://api.example.com/fail", headers={"Authorization": "t"})
resp = send_request(fail_req, [retry_middleware])
assert resp.status == 200
assert resp.body == "recovered"

print("ALL TESTS PASSED")
""",
        "reference": '''
def send_request(request, middlewares):
    from middleware import transport_stub

    def build_chain(idx):
        if idx == len(middlewares):
            return transport_stub
        mw = middlewares[idx]
        nxt = build_chain(idx + 1)
        return lambda req: mw(req, nxt)

    return build_chain(0)(request)
''',
    },
    {
        "id": "state-machine",
        "complexity": "complex",
        "description": "Process state machine events with transitions and actions",
        "files": {
            "states.py": '''from enum import Enum

class State(str, Enum):
    IDLE = "idle"
    RUNNING = "running"
    PAUSED = "paused"
    STOPPED = "stopped"
''',
            "transitions.py": '''from states import State

TRANSITIONS = {
    (State.IDLE, "start"): State.RUNNING,
    (State.RUNNING, "pause"): State.PAUSED,
    (State.RUNNING, "stop"): State.STOPPED,
    (State.PAUSED, "resume"): State.RUNNING,
    (State.PAUSED, "stop"): State.STOPPED,
}
''',
            "actions.py": '''from states import State

def on_start(machine, event):
    machine["runs"] = machine.get("runs", 0) + 1

def on_stop(machine, event):
    machine["stopped"] = True

def on_pause(machine, event):
    machine["paused_at"] = event.get("at")

ACTIONS = {
    (State.RUNNING, "start"): on_start,
    (State.STOPPED, "stop"): on_stop,
    (State.PAUSED, "pause"): on_pause,
}
''',
        },
        "question": """Write `process_event(machine, event) -> State` where `machine` is dict with key "state" (State value) and optional counters. Use TRANSITIONS from transitions.py to determine next state; if no transition, keep current state. Run matching action from ACTIONS in actions.py (keyed by (new_state, event_name)) after transitioning. Update machine["state"] and return the new State.""",
        "test": """
from states import State

m = {"state": State.IDLE}
assert process_event(m, "start") == State.RUNNING
assert m["state"] == State.RUNNING
assert m["runs"] == 1

assert process_event(m, "pause") == State.PAUSED
assert m["paused_at"] is None

assert process_event(m, "resume") == State.RUNNING
assert process_event(m, "stop") == State.STOPPED
assert m["stopped"] is True

noop = {"state": State.IDLE}
assert process_event(noop, "pause") == State.IDLE

print("ALL TESTS PASSED")
""",
        "reference": '''
def process_event(machine, event):
    from actions import ACTIONS
    from states import State
    from transitions import TRANSITIONS

    current = machine["state"]
    event_name = event if isinstance(event, str) else event.get("name")
    payload = {} if isinstance(event, str) else event

    new_state = TRANSITIONS.get((current, event_name), current)
    machine["state"] = new_state

    action = ACTIONS.get((new_state, event_name))
    if action:
        action(machine, payload)

    return new_state
''',
    },
    {
        "id": "data-pipeline",
        "complexity": "complex",
        "description": "Run ETL pipeline across source, transforms, sink",
        "files": {
            "sources.py": '''from typing import Iterator, List

class ListSource:
    def __init__(self, rows: List[dict]):
        self.rows = rows

    def read(self) -> Iterator[dict]:
        yield from self.rows
''',
            "transforms.py": '''from typing import Callable, Iterable

def filter_rows(predicate) -> Callable[[Iterable[dict]], list]:
    def _inner(rows):
        return [r for r in rows if predicate(r)]
    return _inner

def map_rows(fn) -> Callable[[Iterable[dict]], list]:
    def _inner(rows):
        return [fn(r) for r in rows]
    return _inner

def aggregate(key: str) -> Callable[[Iterable[dict]], list]:
    def _inner(rows):
        totals = {}
        for row in rows:
            k = row[key]
            totals[k] = totals.get(k, 0) + row.get("amount", 0)
        return [{"group": k, "total": v} for k, v in sorted(totals.items())]
    return _inner
''',
            "sinks.py": '''from typing import List

class MemorySink:
    def __init__(self):
        self.rows: List[dict] = []

    def write(self, rows: List[dict]):
        self.rows.extend(rows)
''',
        },
        "question": """Write `run_pipeline(source, transforms, sink) -> list` that reads rows from source.read(), applies each transform in order (each transform is a callable taking iterable and returning list), writes final rows to sink.write, and returns the final list.""",
        "test": """
rows = [
    {"region": "eu", "amount": 10},
    {"region": "us", "amount": 5},
    {"region": "eu", "amount": 7},
]
source = ListSource(rows)
sink = MemorySink()
result = run_pipeline(
    source,
    [filter_rows(lambda r: r["amount"] >= 5), aggregate("region")],
    sink,
)
assert result == [{"group": "eu", "total": 17}, {"group": "us", "total": 5}]
assert sink.rows == result

print("ALL TESTS PASSED")
""",
        "reference": '''
def run_pipeline(source, transforms, sink):
    data = list(source.read())
    for transform in transforms:
        data = transform(data)
    sink.write(data)
    return data
''',
    },
    {
        "id": "permission-system",
        "complexity": "complex",
        "description": "Check RBAC access from roles, resources, policies",
        "files": {
            "roles.py": '''from enum import Enum

class Role(str, Enum):
    GUEST = "guest"
    USER = "user"
    ADMIN = "admin"
''',
            "resources.py": '''from enum import Enum

class Resource(str, Enum):
    DOCUMENT = "document"
    BILLING = "billing"
    SETTINGS = "settings"
''',
            "policies.py": '''from resources import Resource
from roles import Role

POLICIES = {
    (Role.GUEST, Resource.DOCUMENT, "read"): True,
    (Role.USER, Resource.DOCUMENT, "read"): True,
    (Role.USER, Resource.DOCUMENT, "write"): True,
    (Role.USER, Resource.BILLING, "read"): True,
    (Role.ADMIN, Resource.DOCUMENT, "write"): True,
    (Role.ADMIN, Resource.BILLING, "write"): True,
    (Role.ADMIN, Resource.SETTINGS, "write"): True,
}

def policy_allows(role, resource, action) -> bool:
    return POLICIES.get((role, resource, action), False)
''',
        },
        "question": """Write `check_access(user_role, resource, action) -> bool` that coerces string inputs to Role/Resource enums when needed and delegates to policy_allows from policies.py.""",
        "test": """
assert check_access("guest", "document", "read") is True
assert check_access("guest", "document", "write") is False
assert check_access(Role.USER, Resource.BILLING, "read") is True
assert check_access("admin", "settings", "write") is True
assert check_access("user", "settings", "write") is False

print("ALL TESTS PASSED")
""",
        "reference": '''
def check_access(user_role, resource, action):
    from policies import policy_allows
    from resources import Resource
    from roles import Role

    role = user_role if isinstance(user_role, Role) else Role(user_role)
    res = resource if isinstance(resource, Resource) else Resource(resource)
    return policy_allows(role, res, action)
''',
    },
    {
        "id": "scheduler",
        "complexity": "complex",
        "description": "Run task schedule respecting dependencies",
        "files": {
            "tasks.py": '''from dataclasses import dataclass, field
from typing import Callable, List

@dataclass
class Task:
    name: str
    deps: List[str] = field(default_factory=list)
    fn: Callable[[], int] = lambda: 0
''',
            "executor.py": '''def execute(task):
    return task.fn()
''',
            "resolver.py": '''from collections import deque
from typing import Dict, List

def resolve_order(tasks: Dict[str, "Task"]) -> List[str]:
    indegree = {name: 0 for name in tasks}
    graph = {name: [] for name in tasks}
    for name, task in tasks.items():
        for dep in task.deps:
            graph[dep].append(name)
            indegree[name] += 1

    queue = deque([n for n, d in indegree.items() if d == 0])
    order = []
    while queue:
        node = queue.popleft()
        order.append(node)
        for nxt in graph[node]:
            indegree[nxt] -= 1
            if indegree[nxt] == 0:
                queue.append(nxt)

    if len(order) != len(tasks):
        raise ValueError("cycle detected")
    return order
''',
        },
        "question": """Write `run_schedule(tasks) -> dict` where `tasks` is a list of Task objects. Build a name->Task dict, resolve execution order via resolve_order, execute each with execute from executor.py, and return {task_name: result} in completion order keys (dict insertion order).""",
        "test": """
t1 = Task("fetch", fn=lambda: 1)
t2 = Task("parse", deps=["fetch"], fn=lambda: 2)
t3 = Task("save", deps=["parse"], fn=lambda: 3)
results = run_schedule([t2, t3, t1])
assert list(results.keys()) == ["fetch", "parse", "save"]
assert results == {"fetch": 1, "parse": 2, "save": 3}

print("ALL TESTS PASSED")
""",
        "reference": '''
def run_schedule(tasks):
    from executor import execute
    from resolver import resolve_order

    task_map = {t.name: t for t in tasks}
    order = resolve_order(task_map)
    results = {}
    for name in order:
        results[name] = execute(task_map[name])
    return results
''',
    },
    {
        "id": "plugin-system",
        "complexity": "complex",
        "description": "Apply registered plugins for a hook",
        "files": {
            "registry.py": '''from typing import Callable, Dict, List

class PluginRegistry:
    def __init__(self):
        self._hooks: Dict[str, List[Callable]] = {}

    def register(self, hook_name: str, fn: Callable):
        self._hooks.setdefault(hook_name, []).append(fn)

    def get_hooks(self, hook_name: str) -> List[Callable]:
        return list(self._hooks.get(hook_name, []))
''',
            "hooks.py": '''HOOK_PROCESS = "process"
HOOK_VALIDATE = "validate"

def validate_positive(value: int) -> bool:
    return value > 0

def double_value(data: dict) -> dict:
    data = dict(data)
    data["value"] = data.get("value", 0) * 2
    return data

def add_metadata(data: dict) -> dict:
    data = dict(data)
    data["meta"] = True
    return data
''',
            "loader.py": '''from hooks import HOOK_PROCESS, HOOK_VALIDATE, add_metadata, double_value, validate_positive

def load_builtin_plugins(registry):
    registry.register(HOOK_VALIDATE, validate_positive)
    registry.register(HOOK_PROCESS, double_value)
    registry.register(HOOK_PROCESS, add_metadata)
''',
        },
        "question": """Write `apply_plugins(registry, data, hook_name) -> Any` that:
1. Gets hook callables via registry.get_hooks(hook_name)
2. For HOOK_VALIDATE (`hooks.HOOK_VALIDATE`): return False if any hook returns falsy, else True (data unchanged)
3. For other hooks: thread `data` through each hook left-to-right (each returns new data) and return final data

Use HOOK_VALIDATE constant from hooks.py to compare hook_name.""",
        "test": """
from hooks import HOOK_PROCESS, HOOK_VALIDATE

reg = PluginRegistry()
load_builtin_plugins(reg)

assert apply_plugins(reg, {"value": 3}, HOOK_VALIDATE) is True
assert apply_plugins(reg, {"value": -1}, HOOK_VALIDATE) is False

out = apply_plugins(reg, {"value": 2}, HOOK_PROCESS)
assert out == {"value": 4, "meta": True}

print("ALL TESTS PASSED")
""",
        "reference": '''
def apply_plugins(registry, data, hook_name):
    from hooks import HOOK_VALIDATE

    hooks = registry.get_hooks(hook_name)
    if hook_name == HOOK_VALIDATE:
        return all(h(data["value"]) for h in hooks)

    result = data
    for hook in hooks:
        result = hook(result)
    return result
''',
    },
]


def strip_local_imports(text: str, file_names) -> str:
    module_names = {fn.replace(".py", "") for fn in file_names}
    lines = []
    for line in text.split("\n"):
        skip = False
        for mod in module_names:
            if f"import {mod}" in line or f"from {mod}" in line:
                skip = True
                break
        if not skip:
            lines.append(line)
    return "\n".join(lines)


def run_task_validation(task: dict) -> tuple[bool, str]:
    file_names = list(task["files"].keys())
    parts = [strip_local_imports(v, file_names) for v in task["files"].values()]
    parts.append(strip_local_imports(task["reference"], file_names))
    parts.append(strip_local_imports(task["test"], file_names))
    script = "\n\n".join(parts) + "\n"

    with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f:
        f.write(script)
        f.flush()
        try:
            result = subprocess.run(
                [PYTHON_BIN, f.name],
                capture_output=True,
                text=True,
                timeout=30,
                env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
            )
            ok = result.returncode == 0 and "ALL TESTS PASSED" in result.stdout
            err = result.stderr or result.stdout
            return ok, err
        finally:
            os.unlink(f.name)


def export_tasks() -> list[dict]:
    exported = []
    for task in TASKS:
        item = {k: v for k, v in task.items() if k != "reference"}
        exported.append(item)
    return exported


def main():
    validate = "--validate" in sys.argv or "-v" in sys.argv

    if validate:
        failed = []
        for task in TASKS:
            ok, err = run_task_validation(task)
            status = "ok" if ok else "FAIL"
            print(f"{task['id']}: {status}")
            if not ok:
                failed.append(task["id"])
                print(err[:500])
        if failed:
            print(f"\nValidation failed: {failed}")
            sys.exit(1)
        print(f"\nAll {len(TASKS)} tasks validated.")
        return

    payload = {
        "version": 2,
        "benchmark": "multifile-context",
        "total_tasks": len(TASKS),
        "tasks": export_tasks(),
    }
    OUTPUT_PATH.write_text(json.dumps(payload, indent=2) + "\n")
    print(f"Wrote {len(TASKS)} tasks to {OUTPUT_PATH}")


if __name__ == "__main__":
    main()
