skill caller() -> Plan
    description: "Returns a call to a same-file export block whose return type differs."

    flow:
        return helper("scope")

export block helper(scope) -> Path
    description: "Helper returning a Path."

    flow:
        "Inspect {scope}."

    return "."
