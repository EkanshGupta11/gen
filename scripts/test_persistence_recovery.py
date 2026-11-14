"""
Automated test for cross-process database persistence and recovery.
Validates:
1. Server starts with an isolated temporary SQLite database.
2. An investigation is submitted through the HTTP API.
3. Result is verified and saved.
4. Server process is cleanly terminated.
5. A new server process is launched pointing to the same temporary database.
6. The exact investigation is retrieved and verified.
7. Temporary database is cleaned up without affecting user data.
"""
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.request
import urllib.error

PORT = int(os.environ.get("PORT", "8787"))
HOST = os.environ.get("HOST", "127.0.0.1")
BASE_URL = f"http://{HOST}:{PORT}"


def wait_for_server(timeout=15):
    start = time.time()
    while time.time() - start < timeout:
        try:
            req = urllib.request.Request(f"{BASE_URL}/health")
            with urllib.request.urlopen(req, timeout=2) as resp:
                if resp.status == 200:
                    data = json.loads(resp.read().decode("utf-8"))
                    if data.get("status") == "ok":
                        return True
        except Exception:
            time.sleep(0.3)
    return False


def http_post(path, data):
    req = urllib.request.Request(
        f"{BASE_URL}{path}",
        data=json.dumps(data).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode("utf-8"))


def http_get(path):
    req = urllib.request.Request(f"{BASE_URL}{path}", method="GET")
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read().decode("utf-8"))


def run_recovery_test():
    exe_path = os.path.join("work", "cargo-target", "debug", "neurogenesis.exe")
    if not os.path.exists(exe_path):
        print(f"Error: binary not found at {exe_path}")
        sys.exit(1)

    temp_dir = tempfile.mkdtemp(prefix="neurogenesis_test_")
    temp_db = os.path.join(temp_dir, "test_recovery.sqlite3")
    print(f"[TEST] Using temporary database: {temp_db}")

    python_exe = os.path.abspath(
        ".venv-cpython313/Scripts/python.exe"
        if os.path.exists(".venv-cpython313/Scripts/python.exe")
        else sys.executable
    )

    env = os.environ.copy()
    env["HOST"] = HOST
    env["PORT"] = str(PORT)
    env["DATABASE_PATH"] = temp_db
    env["PYTHON_EXECUTABLE"] = python_exe
    env["MODEL_PROVIDER"] = "mock"

    # Step 1: Start first server process
    print("[TEST 1/6] Launching initial server process...")
    proc1 = subprocess.Popen(
        [exe_path],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    try:
        if not wait_for_server():
            stdout, stderr = proc1.communicate(timeout=2)
            print(f"Server failed to start:\nStdout: {stdout.decode()}\nStderr: {stderr.decode()}")
            sys.exit(1)
        print("[TEST 2/6] Server 1 is healthy and responding.")

        # Step 2: Create an investigation
        question = "For a projectile launched at speed 20 m/s and angle 35 degrees, estimate the ideal range."
        print(f"[TEST 3/6] Submitting investigation: '{question}'...")
        inv_created = http_post("/api/investigate", {"question": question})
        inv_id = inv_created.get("id")
        assert inv_id, "Investigation ID missing"
        assert inv_created.get("status") == "completed", f"Status not completed: {inv_created.get('status')}"
        assert len(inv_created.get("tools", [])) == 1, "Expected 1 tool execution"
        range_val = inv_created["tools"][0]["output"]["range_m"]
        print(f"  Investigation ID: {inv_id}")
        print(f"  Tool computed range: {range_val:.2f} m")
        print(f"  Status: {inv_created['status']}")

        # Verify it shows up in history on server 1
        history_before = http_post("/api/history", {})
        ids_before = [item["id"] for item in history_before.get("items", [])]
        assert inv_id in ids_before, f"Investigation {inv_id} not found in history before stop"
        print("  Verified investigation exists in server 1 history.")

    finally:
        # Step 3: Stop the first server process
        print("[TEST 4/6] Terminating server 1 process...")
        proc1.terminate()
        try:
            proc1.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc1.kill()
            proc1.wait()
        print("  Server 1 terminated.")

    time.sleep(1.0)

    # Step 4: Start second server process pointing to the SAME temporary database
    print("[TEST 5/6] Launching second server process with same database...")
    proc2 = subprocess.Popen(
        [exe_path],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    try:
        if not wait_for_server():
            stdout, stderr = proc2.communicate(timeout=2)
            print(f"Server 2 failed to start:\nStdout: {stdout.decode()}\nStderr: {stderr.decode()}")
            sys.exit(1)
        print("  Server 2 is healthy and responding.")

        # Step 5: Retrieve the investigation from the new process
        print("[TEST 6/6] Verifying investigation persistence across processes...")
        # Check history list
        history_after = http_post("/api/history", {})
        items_after = history_after.get("items", [])
        matched = [x for x in items_after if x["id"] == inv_id]
        assert len(matched) == 1, f"Expected 1 matching record in history, found {len(matched)}"
        assert matched[0]["status"] == "completed"
        print(f"  History recovery: OK (found record {inv_id})")

        # Check direct GET by ID
        recovered = http_get(f"/api/investigations/{inv_id}")
        assert recovered.get("id") == inv_id, "Recovered investigation ID mismatch"
        assert recovered.get("status") == "completed", "Recovered status mismatch"
        assert len(recovered.get("tools", [])) == 1, "Recovered tools mismatch"
        assert abs(recovered["tools"][0]["output"]["range_m"] - range_val) < 1e-6, "Recovered calculation mismatch"
        assert recovered.get("synthesis"), "Recovered synthesis missing"
        print("  Direct GET /api/investigations/{id} recovery: OK")
        print(f"  Recovered synthesis summary: {recovered['synthesis']['executive_summary']}")
        print("\n>>> CROSS-PROCESS PERSISTENCE & DATABASE RECOVERY TEST: PASSED <<<")

    finally:
        proc2.terminate()
        try:
            proc2.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc2.kill()
            proc2.wait()
        print("  Server 2 terminated.")

        # Cleanup temporary test database files
        for f in os.listdir(temp_dir):
            try:
                os.remove(os.path.join(temp_dir, f))
            except Exception:
                pass
        try:
            os.rmdir(temp_dir)
        except Exception:
            pass
        print("  Temporary test database cleaned up.")


if __name__ == "__main__":
    run_recovery_test()

