require "../runner"

module Commands
  class Fetch < Command
    def git_args(repo : String) : Array(String)
      ["fetch"]
    end

    def format_output(stdout : String, stderr : String, success : Bool) : String
      unless success
        error_line = stderr.each_line.find { |l| !l.strip.empty? } || "unknown error"
        return "ERROR: #{error_line}"
      end

      # git fetch writes progress to stderr, actual updates to stdout
      # If stdout is empty, nothing was fetched
      stdout_lines = stdout.each_line.select { |l| !l.strip.empty? }.to_a
      stderr_lines = stderr.each_line.select { |l| !l.strip.empty? && !l.starts_with?("From") }.to_a

      if stdout_lines.empty? && stderr_lines.empty?
        return "no new commits"
      end

      # Count branches/tags updated from stdout
      updates = stdout.each_line.select { |l| l.includes?("->") || l.includes?("[new") }.to_a

      unless updates.empty?
        branch_count = updates.count { |l| !l.includes?("[new tag]") }
        tag_count = updates.count { |l| l.includes?("[new tag]") }

        parts = [] of String
        if branch_count > 0
          parts << "#{branch_count} branch#{branch_count == 1 ? "" : "es"}"
        end
        if tag_count > 0
          parts << "#{tag_count} tag#{tag_count == 1 ? "" : "s"}"
        end

        unless parts.empty?
          return "#{parts.join(", ")} updated"
        end
      end

      # Fallback
      "fetched"
    end
  end
end
