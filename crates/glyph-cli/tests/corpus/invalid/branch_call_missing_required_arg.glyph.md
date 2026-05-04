skill caller(mode = "fast")
    description: "Calls an export block from a branch body but omits the required argument."

    flow:
        if mode == "fast"
            helper()
        else
            "noop"

export block helper(scope) -> Path
    description: "Helper with a required parameter `scope`."

    flow:
        "Inspect {scope}."

    return "."
