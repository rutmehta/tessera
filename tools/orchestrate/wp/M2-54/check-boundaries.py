"""Source-level guard for the interactive library's blocking boundaries."""
from pathlib import Path
root = Path(__file__).resolve().parents[4]
model = (root / 'apps/mac/Sources/Tessera/Library/LibraryModel.swift').read_text()
assert 'await Task.detached' in model, 'catalog collection still runs on main'
assert 'metadataBatch' in (root / 'apps/mac/Sources/TesseraCore/LibraryCatalog.swift').read_text(), 'metadata reads are not batched'
assert 'cull.isSuggestedBest(item)' in (root / 'apps/mac/Sources/Tessera/App/AppModel.swift').read_text(), 'cells ignore group identity'
print('Interactive boundary guards passed')
