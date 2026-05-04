import "./lib_with_required_export_block.glyph.md" { helper }

skill main()
    description: "Calls an imported export block but omits its required argument."

    flow:
        helper()
