// The domain of your server without the https:// prefix (s1.yourdomain.net)
const SERVER_URL = "YOUR_SERVER_URL";
// Define the target URL
const TARGET_URL = "https://sfgame.net/config.json";

// Listen for requests to the target URL
browser.webRequest.onBeforeRequest.addListener(
  async (details) => {
    // Ensure this request is not triggered by the extension itself
    if (
      details.originUrl != "https://sfgame.net/" &&
      details.type != "main_frame"
    ) {
      return {}; // Ignore requests initiated by the extension
    }

    try {
      // Fetch the original config.json
      const response = await fetch(TARGET_URL);
      const data = await response.json();

      if (Array.isArray(data.servers)) {
        // Add the new server entry
        data.servers.push({
          i: 5020,
          d: SERVER_URL,
          c: "fu",
          p: "2024-11-12 00:00:00",
        });
        data.servers.push({
          i: 5021,
          d: "This server is powered by: ",
          c: "github.com/the-marenga/sf-server",
          md: SERVER_URL,
          m: "2024-11-12 00:00:00",
        });
      }

      // Convert the modified data back to a JSON string
      const modifiedData = JSON.stringify(data);

      // Return the modified response
      return {
        redirectUrl:
          "data:application/json," + encodeURIComponent(modifiedData),
      };
    } catch (error) {
      console.error("Error modifying config.json:", error);
      return {};
    }
  },
  { urls: [TARGET_URL] },
  ["blocking"],
);
