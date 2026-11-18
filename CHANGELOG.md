# Changes

## Unreleased

**Safer**
- The interface may only open, show, copy or read files in the chosen folders, the app's own folder, or files you
  picked or dropped; network paths and programs are refused, and only the kinds of file Effodio reads are opened.
- ffmpeg reads local files only, even an older one on the PATH.
- Downloads give up when a connection stalls and never read past their known size.
- The watcher no longer reads files reached through links, junctions or hidden folders.
- The local door lets go of its port when closed (changing its token no longer leaves it dead) and answers eight
  requests at a time.

**Sturdier**
- A damaged settings file no longer empties the index; it's kept as `config.json.unreadable`.
- A chosen folder that can't be listed for a moment keeps its files, as an unplugged drive does.
- A malformed RTF file, an HTML page full of `&`, or an Office or EPUB file that unpacks to gigabytes no longer stops
  or stalls indexing.
- A search helper that quits no longer fails a search already sent to the next one.
- Views stop listening when you leave them; the shortcut recorder lets go of the keyboard; the large preview hands the
  focus back when it closes; the result lists have names for screen readers.

**Website**
- The feedback form's server reads at most its limit, and answers bad input with an error instead of failing.

## 0.3.5

The release this list starts from.
