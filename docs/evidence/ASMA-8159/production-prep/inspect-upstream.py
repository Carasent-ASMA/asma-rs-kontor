#!/usr/bin/env python3
"""Read immutable public primary source; hashes only, no services/credentials."""
import hashlib,json,urllib.request
from pathlib import Path
PIN="b32d8afc59e1064d9291b9828a8a147be9cc8bab"
FILES=["LICENSE","pyproject.toml","cognee/cli/api_client.py","cognee/modules/users/methods/get_authenticated_user.py","cognee/api/v1/search/routers/get_search_router.py","cognee/modules/pipelines/models/PipelineRunInfo.py"]
rows=[]
for name in FILES:
    url=f"https://raw.githubusercontent.com/topoteretes/cognee/{PIN}/{name}"
    with urllib.request.urlopen(url,timeout=30) as response:
        data=response.read(524289)
        assert len(data)<=524288
    rows.append({"path":name,"url":url,"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()})
Path(__file__).with_name("receipts").joinpath("upstream-primary.json").write_text(json.dumps({"upstream_pin":PIN,"files":rows,"claim":"inspection only; production image/principal/providers unqualified"},indent=2)+"\n")
print(json.dumps(rows,indent=2))
