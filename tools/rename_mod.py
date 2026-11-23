"""Give a mod a new id and name: python tools/rename_mod.py OLD_ID NEW_ID "New Name"

Moves mods/OLD_ID to mods/NEW_ID and its assets/OLD_ID namespace, then replaces the id (also as
`mod-old-id` in Cargo.toml) and the display name in every text file, keeping the lines that point at
the original archived project (`archive/native-v1/mods/OLD_ID.toml`). Prints each changed file.

It replaces the id as plain text, so it also renames any SDK name that happens to contain it (renaming
toggle_sprint once turned the SDK field `options.toggle_sprint` into `options.sprint_sneak_toggle`):
read the diff of src/ before committing, and let CI build it."""
import os
import sys

old, new, new_name = sys.argv[1], sys.argv[2], sys.argv[3]
root = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "mods")
src, dst = os.path.join(root, old), os.path.join(root, new)
assert os.path.isdir(src) and not os.path.exists(dst), "check the folders"

manifest = open(os.path.join(src, "horizon-mod.toml"), encoding="utf-8").read()
old_name = next(l.split("=", 1)[1].strip().strip('"') for l in manifest.splitlines()
                if l.startswith("name = "))

os.rename(src, dst)
if os.path.isdir(os.path.join(dst, "assets", old)):
    os.rename(os.path.join(dst, "assets", old), os.path.join(dst, "assets", new))

KEEP = f"archive/native-v1/mods/{old}.toml"
for folder, dirs, files in os.walk(dst):
    dirs[:] = [d for d in dirs if d not in ("target", ".horizon")]
    for f in files:
        if f.endswith(".png"):
            continue
        p = os.path.join(folder, f)
        text = open(p, encoding="utf-8").read()
        out = text.replace(KEEP, "\0KEEP\0")
        out = out.replace("mod-" + old.replace("_", "-"), "mod-" + new.replace("_", "-"))
        out = out.replace(old, new)
        if f == "horizon-mod.toml":
            out = out.replace(f'name = "{old_name}"', f'name = "{new_name}"')
        out = out.replace("\0KEEP\0", KEEP)
        if out != text:
            open(p, "w", encoding="utf-8", newline="\n").write(out)
            print("changed", os.path.relpath(p, root))
print(f"{old} ({old_name}) -> {new} ({new_name}); the old display name is left in prose for you to edit")
