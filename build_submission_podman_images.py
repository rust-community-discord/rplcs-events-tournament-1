import subprocess
from pathlib import Path
import shutil
import os
import stat

def remove_readonly(func, path, _):
    """Clear the readonly bit and reattempt the removal"""
    os.chmod(path, stat.S_IWRITE)
    func(path)

def safe_remove_tree(path: Path) -> None:
    """Safely remove a directory tree, handling read-only files."""
    if path.exists():
        shutil.rmtree(path, onerror=remove_readonly)

def copy_directory(src: Path, dst: Path) -> None:
    """Copy directory with specific ignore patterns."""
    def ignore_patterns(path, names):
        return ['.git', '__pycache__', '*.pyc']

    shutil.copytree(src, dst, ignore=shutil.ignore_patterns(*ignore_patterns(None, None)))

def build_submission(submission_path: Path) -> None:
    """Build a single submission using podman."""
    submission_name = submission_path.name
    image_name = f"rplcs-tournament-1/{submission_name}"

    # Copy rplcs_events directory into the submission directory
    rplcs_events_src = Path("../rplcs_events")
    rplcs_events_dst = submission_path / "rplcs_events"

    # Safely remove existing directory if it exists
    safe_remove_tree(rplcs_events_dst)

    if rplcs_events_src.exists():
        copy_directory(rplcs_events_src, rplcs_events_dst)
    else:
        print(f"Error: rplcs_events directory not found at {rplcs_events_src}")
        return

    print(f"Building {submission_name} with image name {image_name}")

    try:
        result = subprocess.run(
            ["podman", "build", "-t", image_name, "."],
            cwd=submission_path,
            check=True,
            capture_output=True,
            text=True
        )
        print(f"Successfully built {submission_name}: {result.stdout}")
    except subprocess.CalledProcessError as e:
        print(f"Failed to build {submission_name}")
        print("Error output:")
        print(e.stderr)
        print("Full output:")
        print(e.output)
        raise
    finally:
        # Clean up the copied rplcs_events directory
        safe_remove_tree(rplcs_events_dst)

def main():
    submissions_dir = Path("submissions")

    if not submissions_dir.exists():
        print(f"Error: submissions directory not found at {submissions_dir}")
        return

    # Get all subdirectories in the submissions folder
    submission_paths = [
        p for p in submissions_dir.iterdir()
        if p.is_dir() and (p / "Dockerfile").exists()
    ]

    if not submission_paths:
        print("No submissions found with Dockerfiles")
        return

    print(f"Found {len(submission_paths)} submissions to build")

    # Build each submission
    failed = []
    for submission_path in submission_paths:
        try:
            build_submission(submission_path)
        except subprocess.CalledProcessError:
            failed.append(submission_path.name)

    # Print summary
    if failed:
        print("\nThe following submissions failed to build:")
        for name in failed:
            print(f"- {name}")
    else:
        print("\nAll submissions built successfully!")

if __name__ == "__main__":
    main()
