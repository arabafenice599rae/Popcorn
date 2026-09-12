"""Check every relative link and anchor across the docs, the way GitHub resolves them."""
import re, os, sys, glob

def slug(text):
    # github-slugger: lowercase, strip punctuation, then spaces -> hyphens with NO collapsing.
    # An em dash leaves the spaces around it, so "A — B" becomes "a--b".
    text = re.sub(r'`([^`]*)`', r'\1', text)
    text = re.sub(r'\[([^\]]*)\]\([^)]*\)', r'\1', text)
    text = re.sub(r'<[^>]+>', '', text)
    text = text.lower().strip()
    text = re.sub(r'[^\w\s-]', '', text)
    return text.replace(' ', '-')

def anchors(path):
    out = set()
    for line in open(path):
        m = re.match(r'^(#{1,6})\s+(.*?)\s*$', line)
        if m:
            out.add(slug(m.group(2)))
    return out

files = ['README.md', 'SPEC.md', 'CONSENSUS-LOCK.md', 'CHANGELOG.md', 'web/README.md'] \
    + sorted(glob.glob('docs/*.md'))
cache, broken, checked = {}, [], 0

for f in files:
    base = os.path.dirname(f)
    for m in re.finditer(r'\[([^\]]+)\]\(([^)]+)\)', open(f).read()):
        target = m.group(2)
        if target.startswith(('http://', 'https://', 'mailto:')):
            continue
        checked += 1
        path, _, anchor = target.partition('#')
        resolved = os.path.normpath(os.path.join(base, path)) if path else f
        if not os.path.exists(resolved):
            broken.append(f'{f}: missing file -> {target}')
            continue
        if anchor and resolved.endswith('.md'):
            cache.setdefault(resolved, anchors(resolved))
            if anchor not in cache[resolved]:
                broken.append(f'{f}: missing anchor -> {target}')

print(f'files: {len(files)}   relative links checked: {checked}')
if broken:
    print(f'\n{len(broken)} BROKEN:')
    for b in broken:
        print('  ' + b)
    sys.exit(1)
print('\nall relative links and anchors resolve')
