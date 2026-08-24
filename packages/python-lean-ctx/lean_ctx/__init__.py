"""Synchronous, dependency-free LeanCTX lifecycle and proxy SDK."""

from .core import LeanCTX, LeanCTXConfig
from .kit import ContextKit
from .profile import TuningProfile
from .receipt import ExecutionReceipt, SavingsInfo
from .receipt import ContextReceipt
from .session import ContextSession
from .engine import (
    ContextFailure,
    ContextMeasurement,
    ContextPlan,
    ContextReceiptLink,
    ContextSource,
    ContextView,
    LocalEngineClient,
    PREVIEW_VERSION,
    RecoveredSource,
)
from .wrap import LeanCtxRun, WrappedAgent
from .proxy import CompressResult, ProxyClient, compress
from .client import LeanCtxClient
from .errors import (
    LeanCtxAuthError,
    LeanCtxConnectionError,
    LeanCtxEngineError,
    LeanCtxEngineExecutionError,
    LeanCtxEngineProtocolError,
    LeanCtxEngineRejected,
    LeanCtxEngineTimeout,
    LeanCtxEngineUnavailable,
    EngineExecutionError,
    EngineProtocolError,
    EngineRejectedError,
    EngineTimeoutError,
    EngineUnavailableError,
    LeanCtxError,
)
from .langchain import LeanCtxRetriever, compress_messages
from .litellm import LeanCtxLiteLLMHandler, compress_request_data
from .llamaindex import LeanCtxNodeParser

__version__ = "1.0.0"
__all__ = [
    "LeanCTX",
    "LeanCTXConfig",
    "ContextKit",
    "TuningProfile",
    "ExecutionReceipt",
    "ContextReceipt",
    "SavingsInfo",
    "ContextSession",
    "ContextSource",
    "ContextView",
    "ContextPlan",
    "ContextMeasurement",
    "ContextFailure",
    "ContextReceiptLink",
    "RecoveredSource",
    "LocalEngineClient",
    "PREVIEW_VERSION",
    "LeanCtxRun",
    "WrappedAgent",
    "CompressResult",
    "ProxyClient",
    "compress",
    "LeanCtxClient",
    "LeanCtxAuthError",
    "LeanCtxConnectionError",
    "LeanCtxError",
    "LeanCtxEngineError",
    "LeanCtxEngineExecutionError",
    "LeanCtxEngineProtocolError",
    "LeanCtxEngineRejected",
    "LeanCtxEngineTimeout",
    "LeanCtxEngineUnavailable",
    "EngineExecutionError",
    "EngineProtocolError",
    "EngineRejectedError",
    "EngineTimeoutError",
    "EngineUnavailableError",
    "LeanCtxRetriever",
    "compress_messages",
    "LeanCtxLiteLLMHandler",
    "compress_request_data",
    "LeanCtxNodeParser",
]
