skill caller()
    description: "Calls a private block but omits its required argument."

    flow:
        bar()

block bar(x)
    description: "Helper with a required parameter `x`."

    flow:
        "Use {x}."
