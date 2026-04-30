skill update_docs()
    description: "Update repository documentation."
    require accuracy
    avoid stale_references
    flow:
        "Scan for docs."
        "Update them."

text accuracy = "Be accurate."
text stale_references = "Stale refs."
