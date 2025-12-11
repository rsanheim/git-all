# frozen_string_literal: true

module Nit
  module Repo
    MAX_REPO_NAME_WIDTH = 24

    # Find all git repositories in the given directory (depth 1).
    # Returns a sorted list of absolute paths to directories containing a .git folder.
    def self.find_git_repos(path = Dir.pwd)
      repos = []

      Dir.each_child(path) do |entry|
        full_path = File.join(path, entry)
        next unless File.directory?(full_path)

        git_dir = File.join(full_path, ".git")
        repos << full_path if File.exist?(git_dir)
      end

      repos.sort
    end

    # Extract just the repository name from a path
    def self.repo_name(path)
      File.basename(path) || "unknown"
    end

    # Format repo name with fixed width: truncate long names, pad short ones
    # Returns string like "[name                    ]" (26 chars total)
    def self.format_repo_name(name)
      display_name = if name.length > MAX_REPO_NAME_WIDTH
                       "#{name[0, MAX_REPO_NAME_WIDTH - 4]}-..."
                     else
                       name
                     end
      "[#{display_name.ljust(MAX_REPO_NAME_WIDTH)}]"
    end
  end
end
