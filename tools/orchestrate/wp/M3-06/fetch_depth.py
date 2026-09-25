"""Fetch the pinned Apache-2.0 Small export into the local SHA-addressed cache."""
import hashlib
from pathlib import Path
import urllib.request

REV = "4472b7362082ad9968fee890ca0f1e5aca36b93d"
SHA = "afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c"
URL = f"https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/{REV}/onnx/model.onnx"
root = Path(__file__).resolve().parent / ".cache" / "depth-registry"
root.mkdir(parents=True, exist_ok=True)
path = root / f"{SHA}.onnx"
if not path.exists():
    temporary = path.with_suffix(".download")
    urllib.request.urlretrieve(URL, temporary)
    assert hashlib.sha256(temporary.read_bytes()).hexdigest() == SHA
    temporary.replace(path)
assert hashlib.sha256(path.read_bytes()).hexdigest() == SHA
print(path)
