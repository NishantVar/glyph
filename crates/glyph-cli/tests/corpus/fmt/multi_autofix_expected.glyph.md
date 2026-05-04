import "@glyph/std" { send, subagent }

skill main()
    description: "Main."
    effects: spawns_agent
    flow:
        send("hello")
        subagent("nested")
