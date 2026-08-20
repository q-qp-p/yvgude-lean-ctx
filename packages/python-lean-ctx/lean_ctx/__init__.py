"""Synchronous, dependency-free LeanCTX lifecycle and proxy SDK."""

from .core import LeanCTX, LeanCTXConfig
from .kit import ContextKit
from .profile import TuningProfile
from .receipt import ExecutionReceipt, SavingsInfo
from .session import ContextSession
from .wrap import LeanCtxRun, WrappedAgent
from .proxy import CompressResult, ProxyClient, compress
from .client import LeanCtxClient
from .errors import LeanCtxAuthError, LeanCtxConnectionError, LeanCtxError
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
    "SavingsInfo",
    "ContextSession",
    "LeanCtxRun",
    "WrappedAgent",
    "CompressResult",
    "ProxyClient",
    "compress",
    "LeanCtxClient",
    "LeanCtxAuthError",
    "LeanCtxConnectionError",
    "LeanCtxError",
    "LeanCtxRetriever",
    "compress_messages",
    "LeanCtxLiteLLMHandler",
    "compress_request_data",
    "LeanCtxNodeParser",
]
