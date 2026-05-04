skill caller() -> Path
    description: "Returns a call to an export block but omits the required argument."

    flow:
        return helper()

export block helper(scope) -> Path
    description: "Helper with a required parameter `scope`."

    flow:
        "Inspect {scope}."

    return "."
