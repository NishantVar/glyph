-- Map *.glyph to the `glyph` filetype.
--
-- Drop this file into your `runtimepath` (e.g.
-- `~/.config/nvim/ftdetect/glyph.lua`) or `require` it from your
-- init script. nvim-treesitter routes highlighting and locals to
-- the `glyph` parser based on this filetype.

vim.filetype.add({
  extension = {
    glyph = "glyph",
  },
})
