block fast_mode()
    description: "When the user wants fast processing."
    flow:
        "Do fast processing."

block slow_mode()
    description: "When the user wants thorough processing."
    flow:
        "Do slow processing."

skill main()
    description: "A skill that branches on mode."
    flow:
        "Prepare the environment."
        if mode == "fast"
            "Do the fast thing."
            "Log performance metrics."
        elif mode == "slow"
            "Do the slow thing."
        else
            "Do the default thing."
