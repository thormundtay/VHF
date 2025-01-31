import logging


def no_matplot(msg: logging.LogRecord):
    return not msg.name.startswith("matplotlib") and not msg.name.startswith("PIL")
