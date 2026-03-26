"""macOS Keychain integration via the `security` CLI.
Ported from Rust src/secrets.rs.
"""
import logging
import subprocess

from .config import KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNTS

logger = logging.getLogger(__name__)


class KeychainError(Exception):
    pass


def _run_security(args: list[str], check: bool = True) -> str:
    """Run a macOS `security` CLI command and return stdout."""
    try:
        result = subprocess.run(
            ["security"] + args,
            capture_output=True,
            text=True,
            timeout=10,
        )
        if check and result.returncode != 0:
            raise KeychainError(
                f"security {' '.join(args[:2])} failed: {result.stderr.strip()}"
            )
        return result.stdout.strip()
    except FileNotFoundError:
        raise KeychainError("macOS `security` CLI not found — Keychain is only available on macOS")
    except subprocess.TimeoutExpired:
        raise KeychainError("Keychain operation timed out")


def get_keychain_password(account: str) -> str:
    """Retrieve a password from the macOS Keychain."""
    output = _run_security([
        "find-generic-password",
        "-s", KEYCHAIN_SERVICE,
        "-a", account,
        "-w",
    ])
    return output


def set_keychain_password(account: str, password: str) -> None:
    """Store or update a password in the macOS Keychain."""
    # Delete existing entry first (ignore errors if it doesn't exist)
    _run_security([
        "delete-generic-password",
        "-s", KEYCHAIN_SERVICE,
        "-a", account,
    ], check=False)

    _run_security([
        "add-generic-password",
        "-s", KEYCHAIN_SERVICE,
        "-a", account,
        "-w", password,
    ])


def delete_keychain_password(account: str) -> None:
    """Delete a password from the macOS Keychain."""
    result = subprocess.run(
        ["security", "delete-generic-password", "-s", KEYCHAIN_SERVICE, "-a", account],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0 and "could not be found" not in result.stderr:
        logger.warning("Failed to delete keychain entry '%s': %s", account, result.stderr.strip())


# Convenience functions

def get_oauth_data() -> tuple[str, str, str]:
    """Returns (access_token, refresh_token, realm_id)."""
    access_token = get_keychain_password(KEYCHAIN_ACCOUNTS["access_token"])
    refresh_token = get_keychain_password(KEYCHAIN_ACCOUNTS["refresh_token"])
    realm_id = get_keychain_password(KEYCHAIN_ACCOUNTS["realm_id"])
    return access_token, refresh_token, realm_id


def store_oauth_data(access_token: str, refresh_token: str, realm_id: str) -> None:
    set_keychain_password(KEYCHAIN_ACCOUNTS["access_token"], access_token)
    set_keychain_password(KEYCHAIN_ACCOUNTS["refresh_token"], refresh_token)
    set_keychain_password(KEYCHAIN_ACCOUNTS["realm_id"], realm_id)


def delete_oauth_data() -> None:
    for key in ["access_token", "refresh_token", "realm_id"]:
        delete_keychain_password(KEYCHAIN_ACCOUNTS[key])


def get_client_credentials() -> tuple[str, str]:
    """Returns (client_id, client_secret)."""
    client_id = get_keychain_password(KEYCHAIN_ACCOUNTS["client_id"])
    client_secret = get_keychain_password(KEYCHAIN_ACCOUNTS["client_secret"])
    return client_id, client_secret


def store_client_credentials(client_id: str, client_secret: str) -> None:
    set_keychain_password(KEYCHAIN_ACCOUNTS["client_id"], client_id)
    set_keychain_password(KEYCHAIN_ACCOUNTS["client_secret"], client_secret)


def get_anthropic_api_key() -> str:
    return get_keychain_password(KEYCHAIN_ACCOUNTS["anthropic_api_key"])


def store_anthropic_api_key(api_key: str) -> None:
    set_keychain_password(KEYCHAIN_ACCOUNTS["anthropic_api_key"], api_key)
