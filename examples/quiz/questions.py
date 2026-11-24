"""Quiz content and scoring shared by the HTTP handlers and the example tests."""
from dataclasses import dataclass


@dataclass(frozen=True)
class Question:
    prompt: str
    choices: tuple[str, ...]
    answer: int

    def __post_init__(self):
        if not self.prompt or len(self.choices) < 2 or not 0 <= self.answer < len(self.choices):
            raise ValueError("A question needs a prompt, choices and a valid answer index")


QUESTIONS = (
    Question("Which planet has the shortest year?", ("Earth", "Mercury", "Mars", "Venus"), 1),
    Question("How many bits make one byte?", ("4", "16", "8", "32"), 2),
    Question("What is the largest ocean?", ("Atlantic", "Indian", "Arctic", "Pacific"), 3),
)


def score_attempt(answers, questions=QUESTIONS):
    if len(answers) != len(questions):
        raise ValueError("Answer each question once.")
    score = 0
    for choice, question in zip(answers, questions):
        if not 0 <= choice < len(question.choices):
            raise ValueError("Choose one of the listed answers.")
        score += choice == question.answer
    return score
