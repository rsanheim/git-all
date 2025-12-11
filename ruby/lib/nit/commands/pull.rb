# frozen_string_literal: true

module Nit
  module Commands
    class Pull
      # Format pull output into a single line summary
      def format(output)
        return error_line(output.stderr) unless output.success

        stdout = output.stdout

        # Check for "Already up to date"
        return "Already up to date" if stdout.include?("Already up to date")

        # Try to extract summary (e.g., "3 files changed, 10 insertions(+), 5 deletions(-)")
        # Note: singular "file changed" when only 1 file
        if (summary_line = stdout.lines.find { |l| l.include?("file changed") || l.include?("files changed") })
          return summary_line.strip
        end

        # Check for fast-forward or merge info
        if (line = stdout.lines.find { |l| l.include?("..") || l.include?("Updating") })
          return line.strip
        end

        # Fallback: first non-empty line of stdout or stderr
        all_lines = stdout.lines + output.stderr.lines
        first_line = all_lines.find { |l| !l.strip.empty? }
        first_line&.strip || "completed"
      end

      # Build git command for pull
      def build_command(repo_path, extra_args = [])
        GitCommand.new(repo_path, "pull", extra_args)
      end

      private

      def error_line(stderr)
        line = stderr.lines.find { |l| !l.strip.empty? } || "unknown error"
        "ERROR: #{line.strip}"
      end
    end
  end
end
