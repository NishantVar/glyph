text some_text = "This is a text declaration, not a block."

skill main()
    description: "Main skill."
    flow:
        if some_text.applies()
            "Do something."
