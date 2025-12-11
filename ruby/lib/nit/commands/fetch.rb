# frozen_string_literal: true

module Nit
  module Commands
    class Fetch
      # Format fetch output into a single line summary
      def format(output)
        return error_line(output.stderr) unless output.success

        # git fetch writes progress to stderr, actual updates to stdout
        stdout_lines = output.stdout.lines.map(&:strip).reject(&:empty?)
        stderr_lines = output.stderr.lines
                             .map(&:strip)
                             .reject { |l| l.empty? || l.start_with?("From") }

        return "no new commits" if stdout_lines.empty? && stderr_lines.empty?

        # Count branches/tags updated from stdout
        updates = output.stdout.lines.select { |l| l.include?("->") || l.include?("[new") }

        if updates.any?
          branch_count = updates.count { |l| !l.include?("[new tag]") }
          tag_count = updates.count { |l| l.include?("[new tag]") }

          parts = []
          parts << "#{branch_count} branch#{branch_count == 1 ? "" : "es"}" if branch_count > 0
          parts << "#{tag_count} tag#{tag_count == 1 ? "" : "s"}" if tag_count > 0

          return "#{parts.join(", ")} updated" if parts.any?
        end

        # Fallback
        "fetched"
      end

      # Build git command for fetch
      def build_command(repo_path, extra_args = [])
        GitCommand.new(repo_path, "fetch", extra_args)
      end

      private

      def error_line(stderr)
        line = stderr.lines.find { |l| !l.strip.empty? } || "unknown error"
        "ERROR: #{line.strip}"
      end
    end
  end
end
