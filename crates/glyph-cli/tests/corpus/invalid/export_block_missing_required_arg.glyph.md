skill caller()
    description: "Calls an export block but omits its required argument."

    flow:
        helper()

export block helper(scope) -> Path
    description: "Helper with a required parameter `scope`."

    flow:
        "Inspect {scope}."

    return "."
