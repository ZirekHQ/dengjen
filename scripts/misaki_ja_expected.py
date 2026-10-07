import sys

from misaki import ja

g2p = ja.JAG2P(version="pyopenjtalk")
for text in sys.argv[1:]:
    out, _ = g2p(text)
    print(f"{text}\t{out[:len(out) // 2]}")
