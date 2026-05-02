skill update_docs()
    description: "Update repository documentation."
    require accuracy
    avoid stale_references
    flow:
        "Scan for docs."
        "Update them."

const accuracy = "Be accurate."
const stale_references = "Stale refs."
