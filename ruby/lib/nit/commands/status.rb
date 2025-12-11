# frozen_string_literal: true

module Nit
  module Commands
    class Status
      # Format status output into a single line summary
      def format(output)
        return error_line(output.stderr) unless output.success

        # Parse porcelain output to count file states
        counts = { modified: 0, added: 0, deleted: 0, renamed: 0, untracked: 0 }

        output.stdout.each_line do |line|
          next if line.length < 2

          index_status = line[0]
          worktree_status = line[1]

          # Untracked files
          if index_status == "?"
            counts[:untracked] += 1
            next
          end

          # Check index status (staged changes)
          case index_status
          when "M" then counts[:modified] += 1
          when "A" then counts[:added] += 1
          when "D" then counts[:deleted] += 1
          when "R" then counts[:renamed] += 1
          end

          # Check worktree status (unstaged changes) - only if not already counted
          if index_status == " "
            case worktree_status
            when "M" then counts[:modified] += 1
            when "D" then counts[:deleted] += 1
            end
          end
        end

        # Build human-readable summary
        return "clean" if counts.values.all?(&:zero?)

        parts = []
        parts << "#{counts[:modified]} modified" if counts[:modified] > 0
        parts << "#{counts[:added]} added" if counts[:added] > 0
        parts << "#{counts[:deleted]} deleted" if counts[:deleted] > 0
        parts << "#{counts[:renamed]} renamed" if counts[:renamed] > 0
        parts << "#{counts[:untracked]} untracked" if counts[:untracked] > 0

        parts.join(", ")
      end

      # Build git command for status
      def build_command(repo_path, extra_args = [])
        # Always use --porcelain for machine-readable output
        args = ["--porcelain"] + extra_args
        GitCommand.new(repo_path, "status", args)
      end

      private

      def error_line(stderr)
        line = stderr.lines.find { |l| !l.strip.empty? } || "unknown error"
        "ERROR: #{line.strip}"
      end
    end
  end
end
