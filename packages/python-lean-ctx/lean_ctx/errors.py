"""Exception hierarchy for the lean-ctx SDK."""


class LeanCtxError(Exception):
    """Base class for every error raised by the lean-ctx SDK."""


class LeanCtxConnectionError(LeanCtxError):
    """The local lean-ctx proxy could not be reached.

    Usually means the daemon is not running (``lean-ctx proxy enable``) or the
    discovered host/port is wrong (override with ``LEAN_CTX_PROXY_URL``).
    """


class LeanCtxAuthError(LeanCtxError):
    """The proxy rejected the request (missing or invalid session token).

    Provide the token explicitly, or export ``LEAN_CTX_PROXY_TOKEN`` so the SDK
    can authenticate against the loopback proxy.
    """


class LeanCtxEngineError(LeanCtxError):
    """Base class for failures in the local Preview Engine transport."""


class LeanCtxEngineUnavailable(LeanCtxEngineError):
    """The local Engine executable could not be started or reached."""


class LeanCtxEngineTimeout(LeanCtxEngineUnavailable):
    """The local Engine exceeded its bounded host deadline."""


class LeanCtxEngineProtocolError(LeanCtxEngineError):
    """The Engine returned malformed, unsupported, or cross-bound data."""


class LeanCtxEngineRejected(LeanCtxEngineError):
    """The Engine returned a valid policy-rejected observation."""

    def __init__(self, message: str, *, view=None) -> None:
        super().__init__(message)
        self.view = view


class LeanCtxEngineExecutionError(LeanCtxEngineError):
    """The Engine returned a valid failed observation."""

    def __init__(self, message: str, *, view=None) -> None:
        super().__init__(message)
        self.view = view


# Short Preview names are retained as ergonomic aliases for host code.
EngineUnavailableError = LeanCtxEngineUnavailable
EngineTimeoutError = LeanCtxEngineTimeout
EngineProtocolError = LeanCtxEngineProtocolError
EngineRejectedError = LeanCtxEngineRejected
EngineExecutionError = LeanCtxEngineExecutionError
